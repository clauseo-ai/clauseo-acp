//! The account behind Codex's login: `account/read` through `codex app-server`, in Clauseo's own
//! home, through `codex_appserver`'s client (the brief, "Codex's commands" and "Status and
//! identity"). Its `email` and `plan_type` go into `SignedIn`; nothing else is read.
//!
//! The app-server starts no thread. It runs with the Own environment and the store argument,
//! like every Codex run but detection (detection's runs with `RunKind::Detect` and neither, in
//! an empty Clauseo folder: [`read_as`]), plus the same `-c` overrides `limits`' reader uses
//! (`crates/limits/src/readers/codex.rs`, `CODEX_OVERRIDES`): with plugins on, an app-server
//! starts a git sync into its home at start-up (#203). The talk is `initialize`, then
//! `account/read`; then stdin is closed and the app-server's exit awaited (it may be writing the
//! login file); its tree is killed at its 20 s limit, on a stop at once, or past the lawyer's
//! grace, and a 0-byte login file it left is removed.
//!
//! Only a `ChatGPT` account is a sign-in this card shows ([`AccountRead::Chatgpt`]): an API key,
//! Amazon Bedrock or an account kind this build does not know is not "Signed in to `ChatGPT`".

use std::ffi::OsString;
use std::path::Path;
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use clauseo_codex_appserver::{ClientInfo, GetAccountResponse, connect};
use clauseo_process::{AgentStdin, LaunchSpec, Launched, Timer};
use futures::FutureExt as _;
use futures::future::join;
use futures::io::{AsyncWrite, AsyncWriteExt as _};

use crate::cancel::{Cancel, Stop};
use crate::signin::env::{RunKind, codex_store_args};
use crate::signin::job::SigninJob;
use crate::signin::stopping::{self, OnStop, Stopping};
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
///
/// However the talk ends (answered, or the job stopped gracefully) the app-server's stdin is
/// closed and its exit awaited, since it may be writing Codex's login file (a token refresh): an
/// idle one exits at once. It is killed only by its limit, the stop at once (the quit's bound)
/// or the lawyer's grace; after such a kill a 0-byte login file in its home is removed
/// (`stopping::after_kill`).
async fn account<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    kind: RunKind<'_>,
    cwd: &Path,
) -> Option<Result<GetAccountResponse, clauseo_codex_appserver::Error>> {
    if cancel.is_cancelled() {
        return None;
    }
    let mut spec = job.spec(app_server_args(kind), job.env(kind, None), APP_SERVER_LIMIT);
    spec.cwd = cwd.to_path_buf();
    if let Some(home) = stopping::codex_home_of(&spec.env) {
        stopping::before_status(
            &home,
            spec.lawyer_home.as_deref(),
            job.now(),
            &job.ticket.agent,
        );
    }
    let env = spec.env.clone();
    let lawyer = spec.lawyer_home.clone();
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
    let stdin = SharedStdin(Arc::new(Mutex::new(stdin)));
    let writer = stdin.clone();
    let talk = async move {
        let (client, notifications, driver) = connect(writer, stdout);
        drop(notifications);
        let calls = async move {
            client.initialize(ClientInfo::clauseo()).await?;
            client.account().await
        };
        Some(join(driver, calls).await.1)
    };
    // stderr is drained and never read.
    let mut errs = pin!(
        async move {
            drop(stderr.drain().await);
            futures::future::pending::<()>().await;
        }
        .fuse()
    );
    let mut limit = pin!(job.timer.after(spec.limit).fuse());
    let answered = {
        let mut talk = pin!(talk.fuse());
        // The lawyer's Cancel ends the talk at once; during the quit an answer on its way is
        // still taken (a confirmation that lands then is recorded), until the kill at the bound.
        let mut cancelled = pin!(
            async {
                cancel.cancelled().await;
                if cancel.how() == Some(Stop::Quit) {
                    futures::future::pending::<()>().await;
                }
            }
            .fuse()
        );
        let mut killed = pin!(cancel.killed().fuse());
        let killed_now = futures::select! {
            answered = talk => Some(answered),
            () = errs => Some(None),
            () = cancelled => Some(None),
            () = killed => None,
            () = limit => None,
        };
        let Some(answered) = killed_now else {
            drop(handle);
            stopping::after_kill(APP_SERVER_STOP, &env, lawyer.as_deref(), &job.ticket.agent);
            return None;
        };
        answered
    };
    // The talk is gone: close stdin, then wait for the exit.
    drop(stdin.clone().close().await);
    let grace = cancel.how().and_then(Stop::grace);
    let mut graced = pin!(
        async {
            match grace {
                Some(grace) => job.timer.after(grace).await,
                None => futures::future::pending::<()>().await,
            }
        }
        .fuse()
    );
    let ended = {
        let mut exited = pin!(handle.exited().fuse());
        let mut killed = pin!(cancel.killed().fuse());
        futures::select! {
            _ = exited => true,
            // Never ends: it only keeps stderr drained while the exit is awaited.
            () = errs => false,
            () = killed => false,
            () = graced => false,
            () = limit => false,
        }
    };
    // Kills what is left of the app-server's tree.
    drop(handle);
    if !ended {
        if let Some(how) = cancel.how() {
            tracing::warn!("{}: codex app-server killed at the bound", how.word());
        }
        stopping::after_kill(APP_SERVER_STOP, &env, lawyer.as_deref(), &job.ticket.agent);
    }
    answered
}

/// The app-server's stop: its stdin closed and its exit awaited; it may write the login file.
const APP_SERVER_STOP: OnStop = OnStop {
    how: Stopping::Wait,
    what: "codex app-server",
    writes: true,
};

/// The app-server's stdin, shared by the client's driver and this module, so the module can
/// close it once the talk is over (dropping `AgentStdin` alone does not close the pipe: the
/// handle holds it too).
#[derive(Clone)]
struct SharedStdin(Arc<Mutex<AgentStdin>>);

impl SharedStdin {
    fn lock(&self) -> std::sync::MutexGuard<'_, AgentStdin> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl AsyncWrite for SharedStdin {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut *self.lock()).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.lock()).poll_flush(cx)
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.lock()).poll_close(cx)
    }
}
