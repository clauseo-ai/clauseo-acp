//! The account behind Codex's login: `account/read` through `codex app-server`, in Clauseo's own
//! home, through `codex_appserver`'s client (the brief, "Codex's commands" and "Status and
//! identity"). Its `email` and `plan_type` go into `SignedIn`; nothing else is read.
//!
//! The app-server starts no thread. It runs with the Own environment and the store argument,
//! like every Codex run but detection, plus the same `-c` overrides `limits`' reader uses
//! (`crates/limits/src/readers/codex.rs`, `CODEX_OVERRIDES`): with plugins on, an app-server
//! starts a git sync into its home at start-up (#203). The talk is `initialize`, then
//! `account/read`; then the client is dropped, so stdin closes, and the run's tree is killed when
//! it ends, at its 10 s limit, or on Cancel.

use std::ffi::OsString;
use std::pin::pin;
use std::time::Duration;

use clauseo_codex_appserver::{ClientInfo, GetAccountResponse, connect};
use clauseo_process::{LaunchSpec, Launched, Timer};
use futures::future::{Either, join, select};

use crate::cancel::Cancel;
use crate::signin::env::{RunKind, codex_store_args};
use crate::signin::job::SigninJob;
use crate::status::{Identity, codex_identity};

/// The most the app-server may take to answer.
pub(crate) const APP_SERVER_LIMIT: Duration = Duration::from_secs(10);

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

/// The account and plan `account/read` gives, or none of either when the app-server does not
/// answer, answers with an error, or names no `ChatGPT` account. Login status already said
/// signed in; the identity only fills in who.
pub(crate) async fn read<T: Timer + Send + Sync>(job: &SigninJob<T>, cancel: &Cancel) -> Identity {
    account(job, cancel)
        .await
        .as_ref()
        .and_then(codex_identity)
        .unwrap_or_default()
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

async fn account<T: Timer + Send + Sync>(
    job: &SigninJob<T>,
    cancel: &Cancel,
) -> Option<GetAccountResponse> {
    let kind = RunKind::Own { home: &job.home };
    let spec = job.spec(app_server_args(kind), job.env(kind, None), APP_SERVER_LIMIT);
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
        join(driver, calls).await.1.ok()
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
