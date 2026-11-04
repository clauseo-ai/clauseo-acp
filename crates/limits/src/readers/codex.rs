//! Codex's reader: `codex app-server` started from codex-acp's install, one `initialize`, then
//! `account/read` and `account/rateLimits/read` sent together, mapped to `AccountLimits`.
//!
//! The app-server runs with Clauseo's own `CODEX_HOME`, forced last over any inherited or settings
//! variable of that name (#160), so it never reads the user's `~/.codex`, and with #160's four
//! categories off as `-c` overrides ([`CODEX_OVERRIDES`], #203). Nothing here reads inside that
//! home.

mod map;

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::pin::pin;
use std::time::Duration;

use clauseo_codex_appserver::{
    Account, ClientInfo, Error as AppServerError, GetAccountRateLimitsParams,
    GetAccountRateLimitsResponse, GetAccountResponse, connect,
};
use clauseo_process::{LaunchSpec, Launcher, Timer};
use clauseo_protocol::AgentKind;
use clauseo_settings::AgentLaunch;
use futures::future::join;
use futures::future::{Either, select};
use futures::{AsyncRead, AsyncWrite};

use super::env::{CODEX_DROPPED, spec_env};
use super::paths;
use super::run::{Ran, Stdout, run};
use super::tap::{StdoutTap, tapped};
use crate::{ReadFailure, ReadOutcome, write};

pub(crate) use map::outcome;

/// How long a read may take before its tree is killed.
pub(crate) const LIMIT: Duration = Duration::from_secs(10);
/// How long `account/read` may trail a good `account/rateLimits/read` before the read goes on
/// without it. Codex 0.162.1's `account/read` runs workspace routing discovery for a `ChatGPT`
/// sign-in (`GET .../wham/accounts/check`, a hard-coded 15 s timeout, no config, cached only in
/// the process), which stalled 2 reads in 20 on G16's Windows VM; the limits never wait on it.
pub(crate) const ACCOUNT_GRACE: Duration = Duration::from_millis(500);
/// The variable that names Codex's home.
const CODEX_HOME: &str = "CODEX_HOME";
/// The `-c` overrides the Codex reader runs `app-server` with, before its subcommand, one per
/// category #160 turns off: apps, plugins, skill MCP installs and cloud skills; and, agreed for
/// R15 on #160's reasoning (Clauseo, not Codex, decides what a session loads), Codex's bundled
/// skills and its skills instructions. #160 names the categories, not keys: these come from the
/// `@openai/codex` 0.162.1 source (tag `rust-v0.162.1`): `features/src/lib.rs` for `apps`,
/// `plugins` and `skill_mcp_dependency_install`; `config/src/config_toml.rs` for
/// `cloud.skills`; `config/src/skills_config.rs:41-63` for `skills.bundled.enabled` and
/// `skills.include_instructions`. The host's `CODEX_CONFIG` for codex-acp is built from this
/// constant, so both turn off the same ones.
///
/// `skills.include_instructions=false`: G8a found that app-server lists the user's own skills
/// from `~/.agents/skills`, which it finds through `HOME`, not `CODEX_HOME`, and 5 system skills
/// even with bundled skills off: on a lawyer's machine, content Clauseo did not choose. The key
/// (`skills_config.rs:45-47`, read at `core/src/config/mod.rs:4032-4036`) keeps the
/// available-skills catalog out of the model's context: `ext/skills/src/extension.rs:196-198`
/// returns no thread context and `:417` adds no turn fragment without it, and
/// `ext/skills/src/world_state_catalogs.rs:400-402` renders no catalog.
///
/// R20 adds two (decision record 0048): `cli_auth_credentials_store="file"`, so a Codex sign-in
/// keeps its login in `CODEX_HOME/auth.json`, where S18's link rules apply, never in the OS
/// keyring; and `shell_environment_policy.ignore_default_excludes=false`, which keeps Codex's
/// default exclusion of variables whose names hold `KEY`, `SECRET` or `TOKEN` from its shell
/// tools, so a stored `CODEX_API_KEY` does not reach a command the model runs (believed; G7 row
/// 15 checks it at 0.162.1).
///
/// G16's probe of 0.162.1 (#203) found that app-server, with no login, no thread and even an empty
/// home, starts a git sync of `github.com/openai/plugins` into the home at start-up, because
/// plugins are on by default. `features.plugins=false` stops it; nothing the home's config names
/// (MCP servers, plugin and skill MCP servers, hooks) ran before a thread. The others cost
/// nothing.
pub const CODEX_OVERRIDES: [&str; 8] = [
    "features.apps=false",
    "features.plugins=false",
    "features.skill_mcp_dependency_install=false",
    "cloud.skills.enabled=false",
    "skills.bundled.enabled=false",
    "skills.include_instructions=false",
    "cli_auth_credentials_store=\"file\"",
    "shell_environment_policy.ignore_default_excludes=false",
];

/// What the app-server answered.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Answers {
    /// `account/read`'s response; `None` when it trailed good limits by more than
    /// [`ACCOUNT_GRACE`] or failed beside them (a `ChatGPT` sign-in whose email went unread).
    pub(crate) account: Option<GetAccountResponse>,
    /// `account/rateLimits/read`'s response; kept only for a `ChatGPT` sign-in.
    pub(crate) limits: Option<GetAccountRateLimitsResponse>,
}

/// How the talk ended.
#[derive(Debug, Default)]
pub(crate) enum Talked {
    /// Every answer came.
    Answered(Box<Answers>),
    /// A call or the connection failed.
    Failed(AppServerError),
    /// No talk ended (the run never gives this when it reports an exit).
    #[default]
    Nothing,
}

/// One read of Codex's limits: check the paths and the run folder, make the run folder, then run
/// `codex app-server` and talk to it.
pub(crate) async fn read<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    launch: &AgentLaunch,
    run_dir: &Path,
    codex_home: &Path,
    tap: Option<StdoutTap>,
) -> ReadOutcome {
    let spec = match prepare(launch, run_dir, codex_home, std::env::vars_os()) {
        Ok(Prepared::Spec(spec)) => spec,
        Ok(Prepared::SignedOut) => return ReadOutcome::SignedOut,
        Err(message) => return ReadOutcome::Failed(ReadFailure::LaunchFailed(message)),
    };
    let ran = run(
        launcher,
        &spec,
        timer,
        LIMIT,
        |stdin, stdout| async move {
            let talked = talk(stdin, tapped(stdout, AgentKind::Codex, tap), timer).await;
            let rest: Stdout = Box::new(futures::io::empty());
            (talked, rest)
        },
        |talked| matches!(talked, Talked::Answered(_)),
    )
    .await;
    match ran {
        Ran::LaunchFailed(message) => ReadOutcome::Failed(ReadFailure::LaunchFailed(message)),
        Ran::TimedOut => ReadOutcome::Failed(ReadFailure::TimedOut),
        // Every answer came: they stand whatever the exit.
        Ran::Exited {
            talked: Talked::Answered(answers),
            code,
            ..
        } => {
            if code != Some(0) {
                tracing::debug!(
                    ?code,
                    "the Codex app-server exited non-zero after its answers"
                );
            }
            outcome(*answers)
        }
        Ran::Lingered {
            talked: Talked::Answered(answers),
        } => outcome(*answers),
        Ran::Exited {
            code, stderr_tail, ..
        } if code != Some(0) => ReadOutcome::Failed(ReadFailure::Exited { code, stderr_tail }),
        Ran::Exited { talked, .. } | Ran::Lingered { talked } => match talked {
            Talked::Answered(answers) => outcome(*answers),
            Talked::Failed(err) => malformed(failure_reason(&err)),
            Talked::Nothing => malformed("no answer"),
        },
    }
}

/// What [`prepare`] found.
#[derive(Debug)]
pub(crate) enum Prepared {
    /// The app-server's spec.
    Spec(LaunchSpec),
    /// The Codex home is missing: no login can be there, and the app-server would refuse it.
    SignedOut,
}

/// The app-server's spec: settings `command` running `<codex.js> -c <override>... app-server`
/// (the [`CODEX_OVERRIDES`]) in `run_dir`, with
/// the `inherited` environment under the agent's settings variables and `CODEX_HOME` forced to
/// `codex_home`. A missing `codex_home` is [`Prepared::SignedOut`]; one that is a link or not a
/// folder is refused. It is never made, and nothing inside it is read. Makes `run_dir` if it is
/// missing (nothing in Clauseo writes there); on a refused path, home or run folder it writes
/// nothing. Writes no helper.
pub(crate) fn prepare(
    launch: &AgentLaunch,
    run_dir: &Path,
    codex_home: &Path,
    inherited: impl Iterator<Item = (OsString, OsString)>,
) -> Result<Prepared, String> {
    let (located, codex_js) = paths::codex_js(launch).map_err(|err| err.to_string())?;
    if !paths::check_codex_home(codex_home).map_err(|err| err.to_string())? {
        return Ok(Prepared::SignedOut);
    }
    paths::check_run_dir(run_dir).map_err(|err| err.to_string())?;
    write::create_dir_all(run_dir).map_err(|err| err.to_string())?;
    paths::check_run_dir(run_dir).map_err(|err| err.to_string())?;
    let forced = (OsStr::new(CODEX_HOME), codex_home.as_os_str());
    let env = spec_env(&launch.env, inherited, Some(forced), &CODEX_DROPPED);
    let mut args = vec![codex_js.into_os_string()];
    for set in CODEX_OVERRIDES {
        args.extend([OsString::from("-c"), OsString::from(set)]);
    }
    args.push(OsString::from("app-server"));
    LaunchSpec::new(located.program, args, env, run_dir.to_owned())
        .map(Prepared::Spec)
        .map_err(|err| err.to_string())
}

/// The talk over the app-server's stdin and stdout: `initialize` (which sends `initialized`),
/// then `account/read` and `account/rateLimits/read` (without reset-credit details) on the wire
/// together. The driver runs beside the calls; once they end the client is dropped, so the driver
/// closes stdin and ends, and the app-server exits. Notifications are dropped unread.
///
/// Pipelined so a slow `account/read` cannot hold up the limits (see [`ACCOUNT_GRACE`]):
/// - `account/read` first and good: as before, the limits are awaited only for a `ChatGPT`
///   sign-in, and their error fails the talk;
/// - limits first and good (a `ChatGPT` sign-in): `account/read` gets [`ACCOUNT_GRACE`]; late or
///   failed, the answers go without it;
/// - limits failed (signed out, API key, Bedrock, which skip discovery): `account/read` is awaited
///   and decides, and the limits' error fails the talk only for a `ChatGPT` sign-in;
/// - `account/read` failed: good limits stand without it, else its error fails the talk.
///
/// Errors are told apart by which call failed, never by their text.
pub(crate) async fn talk<W, R, T>(stdin: W, stdout: R, timer: &T) -> Talked
where
    W: AsyncWrite + Send + Unpin + 'static,
    R: AsyncRead + Send + Unpin + 'static,
    T: Timer,
{
    let (client, notifications, driver) = connect(stdin, stdout);
    drop(notifications);
    let calls = async move {
        client.initialize(ClientInfo::clauseo()).await?;
        let params = GetAccountRateLimitsParams {
            exclude_reset_credit_details: Some(true),
            ..GetAccountRateLimitsParams::default()
        };
        // `select` polls `account/read` first, so it goes on the wire first, then the limits.
        let account = pin!(client.account());
        let limits = pin!(client.rate_limits(params));
        let answers = match select(account, limits).await {
            Either::Left((Ok(account), limits)) => {
                let limits = if is_chatgpt(&account) {
                    Some(limits.await?)
                } else {
                    None
                };
                Answers {
                    account: Some(account),
                    limits,
                }
            }
            Either::Left((Err(err), limits)) => match limits.await {
                Ok(limits) => without_account(limits),
                Err(_) => return Err(err),
            },
            Either::Right((Ok(limits), account)) => {
                match select(account, pin!(timer.after(ACCOUNT_GRACE))).await {
                    Either::Left((Ok(account), _)) => {
                        let limits = is_chatgpt(&account).then_some(limits);
                        Answers {
                            account: Some(account),
                            limits,
                        }
                    }
                    Either::Left((Err(err), _)) => {
                        tracing::debug!(
                            error = %err.code().as_str(),
                            "account/read failed beside good limits; the read goes on without it"
                        );
                        without_account(limits)
                    }
                    Either::Right(((), _)) => {
                        tracing::debug!(
                            "account/read trailed the limits; the read goes on without it"
                        );
                        without_account(limits)
                    }
                }
            }
            Either::Right((Err(err), account)) => {
                let account = account.await?;
                if is_chatgpt(&account) {
                    return Err(err);
                }
                Answers {
                    account: Some(account),
                    limits: None,
                }
            }
        };
        Ok::<_, AppServerError>(answers)
    };
    let (driven, answered) = join(driver, calls).await;
    if let Err(err) = driven {
        tracing::debug!(error = %err.code().as_str(), "the Codex app-server connection ended");
    }
    match answered {
        Ok(answers) => Talked::Answered(Box::new(answers)),
        Err(err) => Talked::Failed(err),
    }
}

/// Whether `account` is a `ChatGPT` sign-in.
fn is_chatgpt(account: &GetAccountResponse) -> bool {
    matches!(account.account, Some(Account::Chatgpt(_)))
}

/// Good limits without `account/read`'s answer.
fn without_account(limits: GetAccountRateLimitsResponse) -> Answers {
    Answers {
        account: None,
        limits: Some(limits),
    }
}

/// A short reason for a failed talk, with nothing the app-server sent.
fn failure_reason(err: &AppServerError) -> &'static str {
    match err {
        AppServerError::Closed => "the app-server closed the connection",
        AppServerError::Rpc { .. } => "the app-server answered with an error",
        AppServerError::Protocol(_) => "the app-server broke the protocol",
    }
}

/// A malformed answer, with a short reason that never quotes it.
fn malformed(reason: &str) -> ReadOutcome {
    ReadOutcome::Failed(ReadFailure::Malformed(reason.to_owned()))
}

#[cfg(test)]
mod tests;
