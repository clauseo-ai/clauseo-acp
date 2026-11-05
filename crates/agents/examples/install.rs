//! Installs one agent's ACP adapter into the Clauseo data folder and asks it to initialize.
//!
//! `cargo run -p clauseo-agents --example install -- claude-acp|codex-acp`
//!
//! The data folder is the app's (`CLAUSEO_DATA_DIR` replaces it); `HTTPS_PROXY`, when set, is
//! the install's proxy. Prints each state, then the adapter's `initialize` answer, the wall time
//! and the bytes downloaded. Exits non-zero when the install or the answer fails.

use std::collections::BTreeMap;
use std::error::Error;
use std::io::{self, Write};
use std::pin::pin;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clauseo_agents::{
    Agents, AgentsConfig, AgentsEvent, Clock, InstallState, Installed, Platform, SystemClock,
};
use clauseo_process::{
    AsyncIoTimer, LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions, Timer,
};
use clauseo_protocol::AgentId;
use clauseo_settings::{Locations, SystemDirs, SystemEnv};
use futures::future::{Either, select};
use futures::io::BufReader;
use futures::{AsyncBufReadExt, AsyncWriteExt};

/// The one request the adapter gets.
const INITIALIZE: &str = concat!(
    r#"{"jsonrpc":"2.0","id":0,"method":"initialize","#,
    r#""params":{"protocolVersion":1,"clientCapabilities":{}}}"#,
    "\n"
);

/// How long the adapter has to answer `initialize`.
const ANSWER_WAIT: Duration = Duration::from_secs(60);

/// Bytes in an MB, as the install's sizes count them.
const MB: f64 = 1_000_000.0;

type Fallible<T> = Result<T, Box<dyn Error>>;

fn main() -> ExitCode {
    let mut out = io::stdout().lock();
    match run(&mut out) {
        Ok(code) => code,
        Err(error) => {
            // Nothing is left to do when stderr is gone too.
            drop(writeln!(io::stderr().lock(), "error: {error}"));
            ExitCode::FAILURE
        }
    }
}

fn run(out: &mut impl Write) -> Fallible<ExitCode> {
    let clock = SystemClock;
    let started = clock.monotonic();
    let mut args = std::env::args().skip(1);
    let (Some(id), None) = (args.next(), args.next()) else {
        return Err("usage: install <claude-acp|codex-acp>".into());
    };
    let agent = AgentId::new(&id).map_err(|error| format!("{id}: {error}"))?;
    let locations = Locations::resolve(&SystemDirs, &SystemEnv)?;
    let platform = Platform::current().ok_or("this platform has no Node download")?;
    let mut config = AgentsConfig::new(locations.data_dir().to_owned(), platform);
    config.install.proxy = std::env::var("HTTPS_PROXY")
        .or_else(|_unset| std::env::var("https_proxy"))
        .into_iter()
        .find(|proxy| !proxy.is_empty());
    writeln!(out, "data folder: {}", locations.data_dir().display())?;

    let launcher: Arc<dyn Launcher> = Arc::new(PassthroughLauncher::new(SpawnOptions::new()));
    let (agents, events) = Agents::start(config, Arc::clone(&launcher), AsyncIoTimer);
    agents.install(&agent);
    let mut bytes = (0_u64, 0_u64);
    let followed = follow(out, &agents, &agent, &events, &mut bytes);
    let outcome = futures::executor::block_on(followed);
    let ready = match outcome {
        Ok(ready) => ready,
        Err(error) => {
            writeln!(out, "install did not finish: {error}")?;
            None
        }
    };
    let mut code = ExitCode::FAILURE;
    if let Some(installed) = ready {
        writeln!(out, "node: {}", installed.node.display())?;
        writeln!(out, "script: {}", installed.script.display())?;
        match futures::executor::block_on(initialize(launcher.as_ref(), &installed)) {
            Ok(answer) => {
                describe(out, &answer)?;
                code = ExitCode::SUCCESS;
            }
            Err(error) => writeln!(out, "initialize failed: {error}")?,
        }
    }
    let wall = clock.monotonic().saturating_sub(started);
    writeln!(out, "wall time: {:.1} s", wall.as_secs_f64())?;
    writeln!(out, "downloaded: {} of {} bytes", bytes.0, bytes.1)?;
    Ok(code)
}

/// Prints each of `agent`'s states until it is `Ready` (its install given back), stopped or
/// managed by IT (`None`).
async fn follow(
    out: &mut impl Write,
    agents: &Agents,
    agent: &AgentId,
    events: &async_channel::Receiver<AgentsEvent>,
    bytes: &mut (u64, u64),
) -> Fallible<Option<Installed>> {
    let mut last_print: Option<Duration> = None;
    loop {
        // The channel closes only when the worker is gone.
        let event = events
            .recv()
            .await
            .map_err(|_closed| "the install worker stopped")?;
        let AgentsEvent::Install { agent: from, state } = event else {
            continue;
        };
        if &from != agent {
            continue;
        }
        match state {
            InstallState::Missing { download_bytes } => {
                writeln!(
                    out,
                    "missing: about {:.1} MB to download",
                    mb(download_bytes)
                )?;
            }
            InstallState::Waiting { reason, log } => {
                writeln!(out, "waiting ({}): retries by itself", reason.code())?;
                print_log(out, log.lines())?;
            }
            InstallState::Downloading { received, total } => {
                *bytes = (received, total);
                let now = SystemClock.monotonic();
                let due = last_print
                    .is_none_or(|last| now.saturating_sub(last) >= Duration::from_secs(1));
                if due || received == total {
                    last_print = Some(now);
                    writeln!(out, "downloading: {:.1}/{:.1} MB", mb(received), mb(total))?;
                }
            }
            InstallState::Installing => writeln!(out, "installing")?,
            InstallState::Ready { version } => {
                writeln!(out, "ready: {version}")?;
                return Ok(Some(
                    agents.installed(agent).ok_or("ready but not installed")?,
                ));
            }
            InstallState::Failed(failure) => {
                writeln!(out, "failed: {}", failure.kind.code())?;
                if let (Some(need), Some(free)) = (failure.need_mb, failure.free_mb) {
                    writeln!(out, "needs {need} MB, {free} MB free")?;
                }
                print_log(out, failure.log.lines())?;
                return Ok(None);
            }
            InstallState::Paused => {
                writeln!(out, "paused")?;
                return Ok(None);
            }
            InstallState::Managed => {
                writeln!(out, "managed by IT: nothing to install")?;
                return Ok(None);
            }
            other => writeln!(out, "state: {other:?}")?,
        }
    }
}

/// Runs `node <script>`, sends `initialize` and gives back the first line it answers, parsed.
/// Dropping the handle at the end kills the adapter's tree.
async fn initialize(launcher: &dyn Launcher, installed: &Installed) -> Fallible<serde_json::Value> {
    let spec = LaunchSpec::new(
        installed.node.clone(),
        vec![installed.script.clone().into_os_string()],
        BTreeMap::new(),
        installed.folder.clone(),
    )?;
    let Launched {
        mut stdin,
        stdout,
        stderr,
        handle,
        ..
    } = launcher.launch(&spec)?;
    let talk = async {
        stdin.write_all(INITIALIZE.as_bytes()).await?;
        stdin.flush().await?;
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).await?;
        Ok::<String, io::Error>(line)
    };
    // The pump runs beside the talk, or the adapter stalls once its stderr pipe fills.
    let pumped = async {
        drop(stderr.drain().await);
        futures::future::pending::<io::Result<String>>().await
    };
    let exchange = async {
        match select(pin!(talk), pin!(pumped)).await {
            Either::Left((line, _)) | Either::Right((line, _)) => line,
        }
    };
    let timeout = async {
        AsyncIoTimer.after(ANSWER_WAIT).await;
        Err(io::Error::new(io::ErrorKind::TimedOut, "no answer in 60 s"))
    };
    let line = match select(pin!(exchange), pin!(timeout)).await {
        Either::Left((line, _)) | Either::Right((line, _)) => line?,
    };
    drop(handle);
    if line.is_empty() {
        return Err("the adapter closed its stdout without answering".into());
    }
    Ok(serde_json::from_str(&line)?)
}

/// Prints the answer's `agentInfo` name and version, or the `result` object's keys.
fn describe(out: &mut impl Write, answer: &serde_json::Value) -> io::Result<()> {
    if let Some(error) = answer.get("error") {
        return writeln!(out, "initialize error: {error}");
    }
    let result = answer.get("result");
    if let Some(info) = result.and_then(|result| result.get("agentInfo")) {
        let field = |name: &str| {
            info.get(name)
                .and_then(|value| value.as_str())
                .unwrap_or("?")
        };
        return writeln!(out, "agentInfo: {} {}", field("name"), field("version"));
    }
    let keys: Vec<&str> = result
        .and_then(|result| result.as_object())
        .map(|object| object.keys().map(String::as_str).collect())
        .unwrap_or_default();
    writeln!(out, "result keys: {}", keys.join(", "))
}

fn print_log(out: &mut impl Write, lines: &[String]) -> io::Result<()> {
    lines
        .iter()
        .try_for_each(|line| writeln!(out, "  | {line}"))
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a byte count shown in MB with one decimal"
)]
fn mb(bytes: u64) -> f64 {
    bytes as f64 / MB
}
