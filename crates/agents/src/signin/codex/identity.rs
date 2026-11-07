//! The account behind Codex's login: `account/read` through `codex app-server`, in Clauseo's own
//! home, through `codex_appserver`'s client (the brief, "Codex's commands" and "Status and
//! identity"). Its `email` and `plan_type` go into `SignedIn`; nothing else is read.
//!
//! The app-server starts no thread. It runs with the Own environment and the store argument,
//! like every Codex run but detection (detection's runs with `RunKind::Detect` and neither, in
//! an empty Clauseo folder: [`read_as`]), plus the same `-c` overrides `limits`' reader uses
//! (`crates/limits/src/readers/codex.rs`, `CODEX_OVERRIDES`): with plugins on, an app-server
//! starts a git sync into its home at start-up (#203). The talk is `initialize`, then
//! `account/read`; then the client is dropped, so stdin closes, and the run's tree is killed when
//! it ends, at its 20 s limit, or on Cancel.
//!
//! Only a `ChatGPT` account is a sign-in this card shows ([`AccountRead::Chatgpt`]): an API key,
//! Amazon Bedrock or an account kind this build does not know is not "Signed in to `ChatGPT`".

use std::ffi::OsString;
use std::path::Path;
use std::pin::pin;
use std::time::Duration;

use clauseo_codex_appserver::{ClientInfo, GetAccountResponse, connect};
use clauseo_process::{LaunchSpec, Launched, Timer};
use futures::future::{Either, join, select};

use crate::cancel::Cancel;
use crate::signin::env::{RunKind, codex_store_args};
use crate::signin::job::SigninJob;
use crate::status::{Identity, codex_identity};

/// The most the app-server may take to answer: a cold start measured 3.7 s on a Windows VM.
pub(crate) const APP_SERVER_LIMIT: Duration = Duration::from_secs(20);

/// The `-c` overrides of `limits`' Codex reader (`CODEX_OVERRIDES`, #160 and #203), kept equal
/// by hand: `agents` does not depend on `limits`.
const APP_SERVER_OVERRIDES: [&str; 6] = [
    "features.apps=false",
    "features.plugins=false",
    "features.skill_mcp_dependency_install=false",
    "cloud.skills.enabled=false",
    "skills.bundled.enabled=false",
    "skills.include_instructions=false",
];

/// What `account/read` said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AccountRead {
    /// A `ChatGPT` account: its email (`None` when Codex does not know it) and plan.
    Chatgpt(Identity),
    /// An account that is not a `ChatGPT` sign-in: an API key, Amazon Bedrock, or a kind this
    /// build does not know. Never `SignedIn` or `LoginFound` on this card.
    NotChatgpt,
    /// A definite answer that there is no account: `account: null` (signed out), or a JSON-RPC
    /// error to either call (codex-acp's adapter answers `initialize` with -32602): not a login.
    NoAccount,
    /// No answer: the app-server did not start, closed or broke the protocol before answering,
    /// or was cut short by its limit or by Cancel. It tells nothing either way.
    NoAnswer,
}

/// `account/read` with `kind`'s environment and store argument, in `cwd`: Clauseo's own,
/// detection's (no store argument, the lawyer's own `~/.codex`, an empty Clauseo folder) or a
/// reused session's. A `ChatGPT` account with no email is told apart from a read that said no
/// account (`NoAccount`) or got no answer (`NoAnswer`), neither of which is ever a login, and
/// detection never takes either for one with no email.
pub(crate) async fn read_as<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    kind: RunKind<'_>,
    cwd: &Path,
) -> AccountRead {
    let answered = match account(job, cancel, kind, cwd).await {
        Some(Ok(answered)) => answered,
        Some(Err(clauseo_codex_appserver::Error::Rpc { .. })) => return AccountRead::NoAccount,
        Some(Err(_)) | None => return AccountRead::NoAnswer,
    };
    if answered.account.is_none() {
        return AccountRead::NoAccount;
    }
    codex_identity(&answered).map_or(AccountRead::NotChatgpt, AccountRead::Chatgpt)
}

/// The arguments after `codex.js`: the store argument, the overrides, then `app-server`.
pub(crate) fn app_server_args(kind: RunKind<'_>) -> Vec<OsString> {
    let mut args = codex_store_args(kind);
    for set in APP_SERVER_OVERRIDES {
        args.extend([OsString::from("-c"), OsString::from(set)]);
    }
    args.push(OsString::from("app-server"));
    args
}

/// The app-server's answer to `initialize` then `account/read`: the response or the call's
/// error; `None` when it did not start, or was cut short by its limit or by Cancel.
async fn account<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    kind: RunKind<'_>,
    cwd: &Path,
) -> Option<Result<GetAccountResponse, clauseo_codex_appserver::Error>> {
    let mut spec = job.spec(app_server_args(kind), job.env(kind, None), APP_SERVER_LIMIT);
    spec.cwd = cwd.to_path_buf();
    let mut argv = Vec::with_capacity(spec.args.len().saturating_add(1));
    argv.push(spec.script.into_os_string());
    argv.extend(spec.args);
    let launch = LaunchSpec::new(spec.node, argv, spec.env, spec.cwd).ok()?;
    let Launched {
        stdin,
        stdout,
        stderr,
        handle,
        ..
    } = job.launcher.launch(&launch).ok()?;
    let talk = pin!(async move {
        let (client, notifications, driver) = connect(stdin, stdout);
        drop(notifications);
        let calls = async move {
            client.initialize(ClientInfo::clauseo()).await?;
            client.account().await
        };
        Some(join(driver, calls).await.1)
    });
    // stderr is drained and never read.
    let errs = pin!(async move {
        drop(stderr.drain().await);
        futures::future::pending::<()>().await;
    });
    let cancelled = pin!(cancel.cancelled());
    let limit = pin!(job.timer.after(spec.limit));
    let answered = match select(talk, select(errs, select(cancelled, limit))).await {
        Either::Left((answered, _)) => answered,
        Either::Right(_) => None,
    };
    // Kills the app-server's tree if it is still there.
    drop(handle);
    answered
}
