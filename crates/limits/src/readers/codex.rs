//! Codex's reader: `codex app-server` started from codex-acp's install, one `initialize`, one
//! `account/read` and, for a `ChatGPT` sign-in, one `account/rateLimits/read`, mapped to
//! `AccountLimits`.
//!
//! The app-server runs with Clauseo's own `CODEX_HOME`, forced last over any inherited or settings
//! variable of that name (#160), so it never reads the user's `~/.codex`, and with the host's
//! switches off as `-c` overrides ([`OVERRIDES`], #203). Nothing here reads inside that home.

mod map;

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::time::Duration;

use clauseo_codex_appserver::{
    Account, ClientInfo, Error as AppServerError, GetAccountRateLimitsParams,
    GetAccountRateLimitsResponse, GetAccountResponse, connect,
};
use clauseo_process::{LaunchSpec, Launcher, Timer};
use clauseo_protocol::AgentKind;
use clauseo_settings::AgentLaunch;
use futures::future::join;
use futures::{AsyncRead, AsyncWrite};

use super::env::spec_env;
use super::paths;
use super::run::{Ran, Stdout, run};
use super::tap::{StdoutTap, tapped};
use crate::{ReadFailure, ReadOutcome, write};

pub(crate) use map::outcome;

/// How long a read may take before its tree is killed.
pub(crate) const LIMIT: Duration = Duration::from_secs(10);
/// The variable that names Codex's home.
const CODEX_HOME: &str = "CODEX_HOME";
/// The `-c` overrides the app-server runs with, before its subcommand: the switches the host
/// gives codex-acp under #160, apps, plugins, skill MCP installs and cloud skills off. G16's
/// probe of `@openai/codex` 0.162.1 (#203) found that app-server, with no login, no thread and
/// even an empty home, starts a git sync of `github.com/openai/plugins` into the home at
/// start-up, because plugins are on by default. `features.plugins=false` stops it; nothing else
/// the home's config names (MCP servers, plugin and skill MCP servers, hooks) ran before a
/// thread. The others cost nothing and keep the reader's Codex the host's. Keys from the
/// `rust-v0.162.1` source: `features/src/lib.rs` (apps, plugins,
/// `skill_mcp_dependency_install`) and `config/src/config_toml.rs` (`cloud.skills`).
pub(crate) const OVERRIDES: [&str; 4] = [
    "features.apps=false",
    "features.plugins=false",
    "features.skill_mcp_dependency_install=false",
    "cloud.skills.enabled=false",
];

/// What the app-server answered.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Answers {
    /// `account/read`'s response.
    pub(crate) account: GetAccountResponse,
    /// `account/rateLimits/read`'s response; asked only for a `ChatGPT` sign-in.
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
            let talked = talk(stdin, tapped(stdout, AgentKind::Codex, tap)).await;
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
/// (the [`OVERRIDES`]) in `run_dir`, with
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
    let env = spec_env(&launch.env, inherited, Some(forced));
    let mut args = vec![codex_js.into_os_string()];
    for set in OVERRIDES {
        args.extend([OsString::from("-c"), OsString::from(set)]);
    }
    args.push(OsString::from("app-server"));
    LaunchSpec::new(located.program, args, env, run_dir.to_owned())
        .map(Prepared::Spec)
        .map_err(|err| err.to_string())
}

/// The talk over the app-server's stdin and stdout: `initialize` (which sends `initialized`),
/// `account/read`, and for a `ChatGPT` sign-in `account/rateLimits/read` without reset-credit
/// details. The driver runs beside the calls; once they end the client is dropped, so the driver
/// closes stdin and ends, and the app-server exits. Notifications are dropped unread.
pub(crate) async fn talk<W, R>(stdin: W, stdout: R) -> Talked
where
    W: AsyncWrite + Send + Unpin + 'static,
    R: AsyncRead + Send + Unpin + 'static,
{
    let (client, notifications, driver) = connect(stdin, stdout);
    drop(notifications);
    let calls = async move {
        client.initialize(ClientInfo::clauseo()).await?;
        let account = client.account().await?;
        let limits = if matches!(account.account, Some(Account::Chatgpt(_))) {
            let params = GetAccountRateLimitsParams {
                exclude_reset_credit_details: Some(true),
                ..GetAccountRateLimitsParams::default()
            };
            Some(client.rate_limits(params).await?)
        } else {
            None
        };
        Ok::<_, AppServerError>(Answers { account, limits })
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
