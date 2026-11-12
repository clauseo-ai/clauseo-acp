//! The app-server's answers mapped to a [`ReadOutcome`] for Codex.

use clauseo_codex_appserver::{
    Account, ChatgptAccount, GetAccountRateLimitsResponse, PlanType, RateLimitReachedType,
    RateLimitSnapshot, RateLimitWindow,
};
use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AtLimitBehaviour, Billing, BillingProvider, Finite,
    LimitSource, LimitStopReason, LimitWindow, PlanLabel, Timestamp, WindowKind,
};

use super::Answers;
use crate::model::kind_rank;
use crate::readers::claude::{clean, looks_like_email};
use crate::readers::{MAX_LABEL, MAX_WINDOWS};
use crate::words;
use crate::{AccountRead, ReadFailure, ReadOutcome};

/// The `limitId` of Codex's own limit, whose snapshot sets the windows.
const CODEX_LIMIT: &str = "codex";
/// The account ref of an API key account.
const REF_API_KEY: &str = "api-key";
/// The account ref of an Amazon Bedrock account.
const REF_BEDROCK: &str = "bedrock";
/// The account ref of a `ChatGPT` sign-in with no usable email.
const REF_PLAN: &str = "plan";
/// A week, in minutes.
const WEEK_MINS: i64 = 7 * 24 * 60;
/// The shortest and longest month, in minutes: 28 and 31 days.
const MONTH_MINS: std::ops::RangeInclusive<i64> = 28 * 24 * 60..=31 * 24 * 60;

/// The outcome of a read. Without `account/read` (late or failed while the limits came, which
/// implies a `ChatGPT` sign-in) it is the `ChatGPT` outcome with no email, the plan from the
/// snapshot, and [`AccountRead::NotRead`].
pub(crate) fn outcome(answers: Answers) -> ReadOutcome {
    let Some(read) = answers.account else {
        let unread = ChatgptAccount {
            email: None,
            plan_type: PlanType::Unknown(String::new()),
        };
        return chatgpt(&unread, answers.limits.as_ref(), AccountRead::NotRead);
    };
    match read.account {
        None => ReadOutcome::SignedOut,
        Some(Account::ApiKey) => pay_as_you_go(BillingProvider::OpenAi, REF_API_KEY),
        Some(Account::AmazonBedrock(_)) => pay_as_you_go(BillingProvider::Bedrock, REF_BEDROCK),
        Some(Account::Unknown(_)) => {
            ReadOutcome::Failed(ReadFailure::Malformed("unknown account type".to_owned()))
        }
        Some(Account::Chatgpt(account)) => {
            chatgpt(&account, answers.limits.as_ref(), AccountRead::Read)
        }
    }
}

/// A `ChatGPT` sign-in's outcome from `account` and its `limits`.
fn chatgpt(
    account: &ChatgptAccount,
    limits: Option<&GetAccountRateLimitsResponse>,
    account_read: AccountRead,
) -> ReadOutcome {
    let snapshot = limits.and_then(|limits| {
        let by_id = limits
            .rate_limits_by_limit_id
            .as_ref()
            .and_then(|by_id| by_id.get(CODEX_LIMIT));
        let single = &limits.rate_limits;
        by_id.or_else(|| {
            matches!(single.limit_id.as_deref(), None | Some(CODEX_LIMIT)).then_some(single)
        })
    });
    let plan_type = snapshot
        .and_then(|snapshot| snapshot.plan_type.as_ref())
        .unwrap_or(&account.plan_type);
    let email = account
        .email
        .as_deref()
        .filter(|email| looks_like_email(email));
    let mut windows: Vec<LimitWindow> = snapshot
        .into_iter()
        .flat_map(|snapshot| {
            [
                ("primary", &snapshot.primary),
                ("secondary", &snapshot.secondary),
            ]
        })
        .filter_map(|(id, window)| limit_window(id, window.as_ref()?, plan_type))
        .collect();
    windows.extend(model_windows(limits, plan_type));
    windows.sort_by_key(|window| kind_rank(window.kind));
    windows.truncate(MAX_WINDOWS);
    let at_limit = snapshot
        .and_then(|snapshot| snapshot.credits.as_ref())
        .map_or(AtLimitBehaviour::Pauses, |credits| {
            AtLimitBehaviour::Credits {
                has_credits: credits.has_credits || credits.unlimited,
            }
        });
    let limits = AccountLimits {
        agent: AgentKind::Codex,
        account: AccountRef(email.unwrap_or(REF_PLAN).to_owned()),
        plan: words::codex_plan(plan_type.as_str()).map(|plan| PlanLabel(plan.to_owned())),
        billing: Billing::Plan,
        windows,
        at_limit,
        source: LimitSource::Fresh {
            checked_at: Timestamp::ZERO,
        },
    };
    ReadOutcome::Usage {
        limits,
        reached: snapshot.and_then(reached),
        account_read,
    }
}

/// A pay-as-you-go account billed `via`, with the fixed ref `account`.
fn pay_as_you_go(via: BillingProvider, account: &str) -> ReadOutcome {
    let limits = AccountLimits {
        agent: AgentKind::Codex,
        account: AccountRef(account.to_owned()),
        plan: None,
        billing: Billing::PayAsYouGo { via },
        windows: Vec::new(),
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::NotApplicable,
    };
    ReadOutcome::Usage {
        limits,
        reached: None,
        account_read: AccountRead::Read,
    }
}

/// The windows of each limit in `rateLimitsByLimitId` other than Codex's own (a model's
/// bucket), in `limitId` order: each of its two windows as a `Model` row named by the limit's
/// `limitName`, with the id `<limitId>:primary` or `<limitId>:secondary`, labelled by the model
/// and the window's length. A limit with no usable name is left out, as Claude's reader leaves
/// out a `model_scoped` entry with none.
fn model_windows(
    limits: Option<&GetAccountRateLimitsResponse>,
    plan_type: &PlanType,
) -> Vec<LimitWindow> {
    let Some(by_id) = limits.and_then(|limits| limits.rate_limits_by_limit_id.as_ref()) else {
        return Vec::new();
    };
    by_id
        .iter()
        .filter(|(id, _)| id.as_str() != CODEX_LIMIT)
        .filter_map(|(id, snapshot)| {
            let name: String = clean(snapshot.limit_name.as_deref()?)
                .chars()
                .take(MAX_LABEL)
                .collect();
            (!name.is_empty()).then_some((id, snapshot, name))
        })
        .flat_map(|(id, snapshot, name)| {
            [
                ("primary", &snapshot.primary),
                ("secondary", &snapshot.secondary),
            ]
            .into_iter()
            .filter_map(move |(position, window)| {
                let id = format!("{id}:{position}");
                let mut row = limit_window(&id, window.as_ref()?, plan_type)?;
                let label = words::codex_model_label(&name, &row.label);
                row.label = label.chars().take(MAX_LABEL).collect();
                row.kind = WindowKind::Model;
                row.model = Some(name.clone());
                Some(row)
            })
        })
        .collect()
}

/// One window at position `id` (`primary` or `secondary`). Its length, not its position, gives
/// its kind and label.
fn limit_window(id: &str, window: &RateLimitWindow, plan_type: &PlanType) -> Option<LimitWindow> {
    let mins = window.window_duration_mins.filter(|&mins| mins > 0);
    let (kind, label) = match mins {
        Some(300) => (WindowKind::Session, words::CODEX_SESSION.to_owned()),
        Some(WEEK_MINS) => (WindowKind::Weekly, words::CODEX_WEEKLY.to_owned()),
        Some(mins) if MONTH_MINS.contains(&mins) => {
            (WindowKind::Other, words::CODEX_MONTHLY.to_owned())
        }
        Some(mins) => (WindowKind::Other, words::codex_window_label(mins)),
        // T3's fallback: Free and Go have one monthly window.
        None if matches!(plan_type, PlanType::Free | PlanType::Go) => {
            (WindowKind::Other, words::CODEX_MONTHLY.to_owned())
        }
        None => (WindowKind::Other, words::CODEX_USAGE.to_owned()),
    };
    let used = window.used_percent.clamp(0, 100);
    Some(LimitWindow {
        id: id.to_owned(),
        kind,
        label,
        used_percent: Finite::new(f64::from(used))?,
        resets_at: window
            .resets_at
            .filter(|&secs| secs > 0)
            .and_then(|secs| secs.checked_mul(1000))
            .and_then(|ms| Timestamp::new(ms).ok()),
        duration_mins: mins.and_then(|mins| u32::try_from(mins).ok()),
        warning: false,
        model: None,
    })
}

/// The limit the main snapshot says the account reached. The wire names of
/// `RateLimitReachedType` (`codex_appserver`, schema `rust-v0.162.1`):
/// - `workspace_owner_credits_depleted`, `workspace_member_credits_depleted`: the workspace's
///   credits ran out, so `WorkspaceCredits`;
/// - `workspace_owner_usage_limit_reached`, `workspace_member_usage_limit_reached`: a usage limit
///   the workspace set (spend control) was reached, so `WorkspaceSpendCap`;
/// - `rate_limit_reached`, and any value this crate does not name: a plan window, so
///   `PlanWindow`.
///
/// `spendControlReached: true` is `WorkspaceSpendCap` unless the type names credits, and with
/// neither, nothing is reached.
fn reached(snapshot: &RateLimitSnapshot) -> Option<LimitStopReason> {
    let spend_control = snapshot.spend_control_reached == Some(true);
    match &snapshot.rate_limit_reached_type {
        Some(
            RateLimitReachedType::WorkspaceOwnerCreditsDepleted
            | RateLimitReachedType::WorkspaceMemberCreditsDepleted,
        ) => Some(LimitStopReason::WorkspaceCredits),
        Some(
            RateLimitReachedType::WorkspaceOwnerUsageLimitReached
            | RateLimitReachedType::WorkspaceMemberUsageLimitReached,
        ) => Some(LimitStopReason::WorkspaceSpendCap),
        Some(_) | None if spend_control => Some(LimitStopReason::WorkspaceSpendCap),
        Some(RateLimitReachedType::RateLimitReached | RateLimitReachedType::Unknown(_)) => {
            Some(LimitStopReason::PlanWindow)
        }
        None => None,
    }
}
