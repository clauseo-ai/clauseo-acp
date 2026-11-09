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
use futures::FutureExt as _;
use futures::future::{Either, join, select};

use crate::cancel::Cancel;
use crate::signin::account::ReadOrder;
use crate::signin::env::{RunKind, codex_store_args};
use crate::signin::job::SigninJob;
use crate::status::{Identity, codex_identity};

/// The most the app-server may take to answer: a cold start measured 3.7 s on a Windows VM.
pub(crate) const APP_SERVER_LIMIT: Duration = Duration::from_secs(20);

/// How long a forced read's app-server let go by its caller may drain before it is killed:
/// Codex 0.162.1 drains a started request for up to 30 s after its stdin ends, then exits, and
/// its own watchdog ends it by 45 s.
const LET_GO_LIMIT: Duration = Duration::from_secs(60);

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
    match account(job, cancel, kind, cwd, None).await {
        Some(Ok(answered)) => answered_as(&answered),
        Some(Err(clauseo_codex_appserver::Error::Rpc { .. })) => AccountRead::NoAccount,
        Some(Err(_)) | None => AccountRead::NoAnswer,
    }
}

/// What the identity check's `account/read` said (`signin/account.rs`, R17), forced or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Checked {
    /// An answer, or none: as [`read_as`] reads it, but a JSON-RPC error is
    /// [`AccountRead::NoAnswer`] unless [`rpc_error_kind`] says refused.
    Read(AccountRead),
    /// Codex said the login's tokens are refused ([`UNAUTHORIZED`]): a definite auth refusal.
    Refused,
}

/// The start of the JSON-RPC error message Codex 0.162.1 gives when `account/read`'s workspace
/// routing discovery is refused by `OpenAI` (a revoked or refused login), code `-32603`:
/// `codex-rs`' `workspace_routing.rs:64` (`"workspace routing discovery unauthorized (401)"`).
pub(crate) const UNAUTHORIZED: &str = "workspace routing discovery unauthorized";

/// Codex's JSON-RPC internal error code, which both of `workspace_routing.rs`' errors carry.
const INTERNAL_ERROR: i64 = -32603;

/// The identity check's `account/read` with `kind`'s environment and store argument, in `cwd`,
/// with `refreshToken: true` when `order.refresh` (R17's forced refresh: Codex refreshes the tokens
/// with `OpenAI` before it answers, in the same request; 0.162.1 then answers from the stored
/// `id_token` whatever the refresh's outcome). A JSON-RPC error is [`rpc_error_kind`]'s.
pub(crate) async fn read_checked<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    kind: RunKind<'_>,
    cwd: &Path,
    order: &ReadOrder,
) -> Checked {
    match account(job, cancel, kind, cwd, Some(order)).await {
        Some(Ok(answered)) => Checked::Read(answered_as(&answered)),
        Some(Err(clauseo_codex_appserver::Error::Rpc { code, message })) => {
            rpc_error_kind(code, &message)
        }
        Some(Err(_)) | None => Checked::Read(AccountRead::NoAnswer),
    }
}

/// What a JSON-RPC error to the identity check's `account/read` (forced or not) says: the one
/// mapping. Codex 0.162.1's `-32603` [`UNAUTHORIZED`] (`workspace_routing.rs:64`) is a refused
/// login; everything else is no answer, never `NoAccount`: its `-32603` `"workspace routing
/// discovery failed"` (`workspace_routing.rs:62`) is what an offline app-server gives, forced
/// or not, and any other error tells nothing either. The message is never logged or kept.
pub(crate) fn rpc_error_kind(code: i64, message: &str) -> Checked {
    if code == INTERNAL_ERROR && message.starts_with(UNAUTHORIZED) {
        Checked::Refused
    } else {
        Checked::Read(AccountRead::NoAnswer)
    }
}

/// An answer's account: none, a `ChatGPT` one, or another kind.
fn answered_as(answered: &GetAccountResponse) -> AccountRead {
    if answered.account.is_none() {
        return AccountRead::NoAccount;
    }
    codex_identity(answered).map_or(AccountRead::NotChatgpt, AccountRead::Chatgpt)
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
/// error; `None` when it did not start, or was cut short by its limit, by Cancel, or by its
/// caller letting it go. With `order.refresh` (a forced read), `account/read` asks for a token
/// refresh first (no other request is added), and:
///
/// - one forced read's app-server at a time per home
///   ([`crate::signin::account::ForcedRuns`]): while one given up still drains, another gets no
///   answer without starting;
/// - let go by its caller (the identity check's bound), its stdin is closed and it is not
///   killed: Codex may have rotated the refresh token and not yet saved it. It drains its
///   started request and exits on its own, its reply read and dropped, and is reaped; a quit
///   (Cancel) or [`LET_GO_LIMIT`] kills it.
async fn account<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    kind: RunKind<'_>,
    cwd: &Path,
    order: Option<&ReadOrder>,
) -> Option<Result<GetAccountResponse, clauseo_codex_appserver::Error>> {
    let forced = order.filter(|order| order.refresh);
    let _run = match forced {
        Some(order) if order.let_go.is_cancelled() => return None,
        Some(order) => Some(order.forced.claim(cwd)?),
        None => None,
    };
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
        mut handle,
        ..
    } = job.launcher.launch(&launch).ok()?;
    let refresh = forced.is_some();
    let mut talk = pin!(async move {
        let (client, notifications, driver) = connect(stdin, stdout);
        drop(notifications);
        let calls = async move {
            client.initialize(ClientInfo::clauseo()).await?;
            if refresh {
                client.account_refreshed().await
            } else {
                client.account().await
            }
        };
        Some(join(driver, calls).await.1)
    });
    // stderr is drained and never read.
    let mut errs = pin!(async move {
        drop(stderr.drain().await);
        futures::future::pending::<()>().await;
    });
    let mut cancelled = pin!(cancel.cancelled());
    let mut limit = pin!(job.timer.after(spec.limit));
    let mut let_go = pin!(async move {
        match forced {
            Some(order) => order.let_go.cancelled().await,
            None => futures::future::pending::<()>().await,
        }
    });
    let first = {
        let answered = talk.as_mut().map(First::Answered);
        let gone = let_go.as_mut().map(|()| First::LetGo);
        let over = select(errs.as_mut(), select(cancelled.as_mut(), limit.as_mut()));
        let over = over.map(|_| First::Over);
        match select(answered, select(gone, over)).await {
            Either::Left((first, _))
            | Either::Right((Either::Left((first, _)) | Either::Right((first, _)), _)) => first,
        }
    };
    let answered = match first {
        First::Answered(answered) => answered,
        First::Over => None,
        First::LetGo => {
            handle.close_stdin();
            let ended = pin!(async { join(talk.as_mut(), handle.exited()).await });
            let drained = pin!(job.timer.after(LET_GO_LIMIT));
            let quit = select(cancelled.as_mut(), drained);
            let _ended = select(ended, select(errs.as_mut(), quit)).await;
            None
        }
    };
    // Kills the app-server's tree if it is still there.
    drop(handle);
    answered
}

/// How a run's first wait ended.
enum First<A> {
    Answered(A),
    /// Its limit or Cancel (or stderr's end, which never comes first).
    Over,
    /// Its caller gave it up.
    LetGo,
}

#[cfg(test)]
mod tests;
