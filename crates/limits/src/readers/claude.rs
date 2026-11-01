//! Claude's reader: the embedded helper, its launch, and its one line mapped to
//! `AccountLimits`.
//!
//! The helper (`helper/claude-usage.mjs`) runs under the adapter's own node and SDK, so the
//! vendor's Claude Code process reads its own login. Its line is untrusted: unknown fields are
//! ignored, and a field of the wrong type drops only what it holds.

use std::path::Path;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launcher, Timer};
use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AtLimitBehaviour, Billing, BillingProvider, Finite,
    LimitSource, LimitWindow, PlanLabel, Timestamp, UnavailableReason, WindowKind,
};
use clauseo_settings::AgentLaunch;
use serde_json::{Map, Value};

use super::env::spec_env;
use super::paths;
use super::run::{Line, Ran, first_line, run};
use super::time::parse_rfc3339;
use crate::words::{self, CLAUDE_WINDOWS};
use crate::{ReadFailure, ReadOutcome, write};

/// The helper's bytes, written into the install folder when the file there differs.
pub(crate) const HELPER: &str = include_str!("../../helper/claude-usage.mjs");
/// How long a read may take before its tree is killed.
pub(crate) const LIMIT: Duration = Duration::from_secs(25);
/// The most windows one read keeps.
const MAX_WINDOWS: usize = 16;
/// The most chars a label keeps.
const MAX_LABEL: usize = 64;

/// The account ref of a pay-as-you-go account with no usable email: any API key provider.
const REF_API_KEY: &str = "api-key";
/// The account ref of a Bedrock account with no usable email.
const REF_BEDROCK: &str = "bedrock";
/// The account ref of a Vertex account with no usable email.
const REF_VERTEX: &str = "vertex";
/// The account ref of a subscription with no usable email.
const REF_PLAN: &str = "plan";

/// One read of Claude's limits: check the paths, write the helper if it differs, make the
/// working folder, then run the helper and read its line.
pub(crate) async fn read<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    launch: &AgentLaunch,
    run_dir: &Path,
) -> ReadOutcome {
    let spec = match prepare(launch, run_dir) {
        Ok(spec) => spec,
        Err(message) => return ReadOutcome::Failed(ReadFailure::LaunchFailed(message)),
    };
    let ran = run(launcher, &spec, timer, LIMIT, |_stdin, stdout| {
        first_line(stdout)
    })
    .await;
    match ran {
        Ran::LaunchFailed(message) => ReadOutcome::Failed(ReadFailure::LaunchFailed(message)),
        Ran::TimedOut => ReadOutcome::Failed(ReadFailure::TimedOut),
        Ran::Exited {
            code, stderr_tail, ..
        } if code != Some(0) => ReadOutcome::Failed(ReadFailure::Exited { code, stderr_tail }),
        Ran::Exited {
            talked: Line::Missing,
            ..
        } => malformed("no line"),
        Ran::Exited {
            talked: Line::TooLong,
            ..
        } => malformed("the line is over 1 MiB"),
        Ran::Exited {
            talked: Line::Read(line),
            stderr_tail,
            ..
        } => match answer(&line) {
            Answer::Outcome(outcome) => outcome,
            Answer::HelperFailed => ReadOutcome::Failed(ReadFailure::Exited {
                code: Some(0),
                stderr_tail,
            }),
        },
    }
}

/// The helper's spec: settings `command` running the helper, in `run_dir`, with the agent's
/// environment. Writes the helper (only when it differs) and makes `run_dir` first; on a refused
/// path it writes nothing.
fn prepare(launch: &AgentLaunch, run_dir: &Path) -> Result<LaunchSpec, String> {
    let (located, helper) = paths::claude(launch).map_err(|err| err.to_string())?;
    write::replace_if_differs(&helper, HELPER.as_bytes()).map_err(|err| err.to_string())?;
    write::create_dir_all(run_dir).map_err(|err| err.to_string())?;
    let env = spec_env(&launch.env, std::env::vars_os(), None);
    LaunchSpec::new(
        located.program,
        vec![helper.into_os_string()],
        env,
        run_dir.to_owned(),
    )
    .map_err(|err| err.to_string())
}

/// What the helper's line says.
#[derive(Debug, PartialEq)]
pub(crate) enum Answer {
    /// The read's outcome.
    Outcome(ReadOutcome),
    /// `ok:false` with `missing:false`: the helper failed, and wrote why to stderr. The read
    /// reports the redacted stderr tail, never the line's own message.
    HelperFailed,
}

/// A malformed line, with a short reason that never quotes the line.
fn malformed(reason: &str) -> ReadOutcome {
    ReadOutcome::Failed(ReadFailure::Malformed(reason.to_owned()))
}

/// Reads the helper's line (without its `\n`).
pub(crate) fn answer(line: &[u8]) -> Answer {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Answer::Outcome(malformed("the line is not JSON"));
    };
    let Some(line) = value.as_object() else {
        return Answer::Outcome(malformed("the line is not a JSON object"));
    };
    match line.get("ok") {
        Some(Value::Bool(true)) => Answer::Outcome(usage(line)),
        Some(Value::Bool(false)) => failure(line),
        _ => Answer::Outcome(malformed("the line has no ok field")),
    }
}

/// An `ok:false` line. Its `message` is not redacted, so only its length is logged.
fn failure(line: &Map<String, Value>) -> Answer {
    let Some(missing) = line.get("missing").and_then(Value::as_bool) else {
        return Answer::Outcome(malformed("an ok:false line has no missing field"));
    };
    let stage = line
        .get("stage")
        .and_then(Value::as_str)
        .filter(|stage| matches!(*stage, "init" | "usage"))
        .unwrap_or("unknown");
    let message_len = line
        .get("message")
        .and_then(Value::as_str)
        .map_or(0, str::len);
    tracing::debug!(
        stage,
        missing,
        message_len,
        "the Claude usage helper answered ok:false"
    );
    if missing {
        Answer::Outcome(ReadOutcome::MethodMissing)
    } else {
        Answer::HelperFailed
    }
}

/// An `ok:true` line. A `usage` that does not fit the schema means the usage method is missing
/// or reshaped, which is not an error.
fn usage(line: &Map<String, Value>) -> ReadOutcome {
    let account = line.get("account").and_then(Value::as_object);
    let Some(usage) = line.get("usage").and_then(Value::as_object) else {
        return ReadOutcome::MethodMissing;
    };
    let Some(available) = usage.get("rate_limits_available").and_then(Value::as_bool) else {
        return ReadOutcome::MethodMissing;
    };
    let subscription = match usage.get("subscription_type") {
        None | Some(Value::Null) => None,
        Some(Value::String(subscription)) => Some(subscription.as_str()),
        Some(_) => return ReadOutcome::MethodMissing,
    };
    let rate_limits = match usage.get("rate_limits") {
        None | Some(Value::Null) => None,
        Some(Value::Object(rate_limits)) => Some(rate_limits),
        Some(_) => return ReadOutcome::MethodMissing,
    };
    let text = |key: &str| {
        account
            .and_then(|account| account.get(key))
            .and_then(Value::as_str)
    };
    let email = text("email").filter(|email| looks_like_email(email));
    let limits = if subscription.is_none() && !available {
        let via = provider(text("apiProvider"));
        let fixed = match via {
            BillingProvider::Bedrock => REF_BEDROCK,
            BillingProvider::Vertex => REF_VERTEX,
            _ => REF_API_KEY,
        };
        AccountLimits {
            agent: AgentKind::Claude,
            account: account_ref(email, fixed),
            plan: None,
            billing: Billing::PayAsYouGo { via },
            windows: Vec::new(),
            at_limit: AtLimitBehaviour::Pauses,
            source: LimitSource::NotApplicable,
        }
    } else {
        let (windows, source) = if available {
            (
                windows(rate_limits),
                LimitSource::Fresh {
                    checked_at: Timestamp::ZERO,
                },
            )
        } else {
            let reason = UnavailableReason::MissingScope;
            (Vec::new(), LimitSource::Unavailable { reason })
        };
        AccountLimits {
            agent: AgentKind::Claude,
            account: account_ref(email, REF_PLAN),
            plan: subscription
                .and_then(words::claude_plan)
                .map(|plan| PlanLabel(plan.to_owned())),
            billing: Billing::Plan,
            windows,
            at_limit: at_limit(rate_limits),
            source,
        }
    };
    ReadOutcome::Usage {
        limits,
        reached: None,
    }
}

/// Who bills a pay-as-you-go account, from `account.apiProvider`.
fn provider(api_provider: Option<&str>) -> BillingProvider {
    match api_provider {
        None | Some("firstParty") => BillingProvider::Anthropic,
        Some("bedrock" | "anthropicAws") => BillingProvider::Bedrock,
        Some("vertex" | "anthropicGoogleCloud") => BillingProvider::Vertex,
        Some(_) => BillingProvider::Other,
    }
}

/// The email when there is one, else the fixed ref. Never a key or a token.
fn account_ref(email: Option<&str>, fixed: &str) -> AccountRef {
    AccountRef(email.unwrap_or(fixed).to_owned())
}

/// Whether `text` looks like an email: 3 to 254 bytes, exactly one `@` with text on both sides,
/// and no whitespace or control characters.
pub(crate) fn looks_like_email(text: &str) -> bool {
    (3..=254).contains(&text.len())
        && text.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        })
        && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// What happens at the limit: extra usage when `extra_usage` is an object with a boolean
/// `is_enabled`, else the agent pauses.
fn at_limit(rate_limits: Option<&Map<String, Value>>) -> AtLimitBehaviour {
    rate_limits
        .and_then(|rate_limits| rate_limits.get("extra_usage"))
        .and_then(Value::as_object)
        .and_then(|extra| extra.get("is_enabled"))
        .and_then(Value::as_bool)
        .map_or(AtLimitBehaviour::Pauses, |enabled| {
            AtLimitBehaviour::ExtraUsage { enabled }
        })
}

/// The windows in `rate_limits`, sorted session, weekly, model, other, at most
/// [`MAX_WINDOWS`]. A window whose utilization is missing or not a number is left out.
fn windows(rate_limits: Option<&Map<String, Value>>) -> Vec<LimitWindow> {
    let Some(rate_limits) = rate_limits else {
        return Vec::new();
    };
    let mut windows: Vec<LimitWindow> = CLAUDE_WINDOWS
        .iter()
        .filter_map(|fixed| {
            let value = rate_limits.get(fixed.key)?;
            window(
                value,
                fixed.key.to_owned(),
                fixed.kind,
                fixed.label,
                fixed.duration_mins,
            )
        })
        .collect();
    if let Some(Value::Array(models)) = rate_limits.get("model_scoped") {
        let single = models.len() == 1;
        let scoped = models.iter().enumerate().filter_map(|(n, entry)| {
            let name = clean(entry.get("display_name")?.as_str()?);
            if name.is_empty() {
                return None;
            }
            let id = if single {
                words::CLAUDE_OVERAGE_INCLUDED.to_owned()
            } else {
                format!("{}{n}", words::CLAUDE_MODEL_SCOPED)
            };
            let label = words::claude_model_label(&name);
            window(
                entry,
                id,
                WindowKind::Model,
                &label,
                words::CLAUDE_MODEL_SCOPED_MINS,
            )
        });
        windows.extend(scoped.take(MAX_WINDOWS));
    }
    windows.sort_by_key(|window| rank(window.kind));
    windows.truncate(MAX_WINDOWS);
    windows
}

/// One window from its `{utilization, resets_at}` object; `None` when `value` is not an object
/// or its utilization is not a number. Utilization is clamped to 0 to 100; a reset time that is
/// not RFC 3339 is left out.
fn window(
    value: &Value,
    id: String,
    kind: WindowKind,
    label: &str,
    duration_mins: u32,
) -> Option<LimitWindow> {
    let value = value.as_object()?;
    let utilization = value.get("utilization")?.as_f64()?;
    let used = if utilization > 100.0 {
        100.0
    } else if utilization > 0.0 {
        utilization
    } else {
        0.0
    };
    let resets_at = value
        .get("resets_at")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339);
    Some(LimitWindow {
        id,
        kind,
        label: label.chars().take(MAX_LABEL).collect(),
        used_percent: Finite::new(used)?,
        resets_at,
        duration_mins: Some(duration_mins),
        warning: false,
    })
}

/// A server label with control characters turned into spaces and its ends trimmed.
fn clean(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    cleaned.trim().to_owned()
}

/// The sort rank of a window kind: session, weekly, model, other.
fn rank(kind: WindowKind) -> u8 {
    match kind {
        WindowKind::Session => 0,
        WindowKind::Weekly => 1,
        WindowKind::Model => 2,
        WindowKind::Other => 3,
    }
}

#[cfg(test)]
mod tests;
