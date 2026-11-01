//! The readers: one read of an agent's plan usage through its vendor's own tool.
//!
//! `paths.rs` finds and checks the adapter's install folder, `env.rs` builds the environment,
//! `run.rs` runs a program under a deadline, and `claude.rs` is Claude's reader. The Codex reader
//! (`codex.rs`) follows R14.

mod claude;
mod env;
mod paths;
mod run;
mod time;

use std::future::{Future, ready};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::{AccountLimits, AgentKind, LimitStopReason};
use clauseo_settings::{AgentLaunch, Effective};

use paths::CLAUDE_ID;

/// How a read ended.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadOutcome {
    /// The agent's usage. A reader sets `limits.source` to `NotApplicable` or
    /// `Unavailable { MissingScope }` when that is the answer; the model overwrites any other value
    /// with `Fresh { checked_at: now }`.
    Usage {
        /// The limits read.
        limits: AccountLimits,
        /// The limit the vendor says the account has reached (Codex's `rateLimitReachedType`
        /// and `spendControlReached`); always `None` for Claude.
        reached: Option<LimitStopReason>,
    },
    /// The usage method is missing or reshaped (Claude only): the source is unavailable, which
    /// is not an error.
    MethodMissing,
    /// The read failed.
    Failed(ReadFailure),
}

/// Why a read failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadFailure {
    /// The reader did not answer in time; its tree was killed.
    TimedOut,
    /// The reader did not start.
    LaunchFailed(String),
    /// The reader exited without its answer.
    Exited {
        /// Its exit code; `None` when a signal ended it.
        code: Option<i32>,
        /// The end of its stderr, redacted by `process`.
        stderr_tail: String,
    },
    /// Its answer did not read.
    Malformed(String),
}

/// Reads one agent's limits.
pub trait ReadLimits: Send + Sync {
    /// Reads `agent`'s limits once.
    fn read(&self, agent: AgentKind) -> Pin<Box<dyn Future<Output = ReadOutcome> + Send>>;
}

/// [`ReadLimits`] over a [`Launcher`] and a [`Timer`].
///
/// Each read checks the adapter's paths again, writes the helper into its install folder when
/// the file there differs, makes the working folder `<data_dir>/limits/run` if it is missing
/// (nothing in Clauseo writes there), and runs the helper with the inherited environment under
/// the agent's settings variables.
///
/// The read future does a little blocking file work before the launch, on its first poll: the
/// path checks, the compare of a ~4 KiB file, at most one small write, and the folder creation.
/// Poll it where a brief block is fine.
pub struct ProcessReaders<T> {
    launcher: Arc<dyn Launcher>,
    timer: T,
    /// Claude's settings entry, if it has one. Never `Debug`-printed: its env may hold secrets.
    claude: Option<AgentLaunch>,
    /// The readers' working folder, `<data_dir>/limits/run`.
    run_dir: PathBuf,
    /// Clauseo's own Codex home, which the Codex reader forces as `CODEX_HOME`.
    codex_home: PathBuf,
}

impl<T: Timer + Clone + Send + Sync + 'static> ProcessReaders<T> {
    /// Readers for the agents `effective` has entries for. A Claude entry whose paths break a
    /// rule is logged once here, at `warn`, and each of its reads fails with the rule.
    ///
    /// `codex_home` is Clauseo's own Codex home, required and with no default. The Codex reader
    /// sets `CODEX_HOME` to it last, over any inherited or settings variable of that name in any
    /// case: codex-acp merges the user's `~/.codex/config.toml` (MCP servers, plugins, hooks,
    /// rules) unless `CODEX_HOME` is a separate folder, so Clauseo owns one, and Codex signs in
    /// separately inside Clauseo. Nothing here reads inside it. The Claude reader ignores it.
    pub fn new(
        effective: &Effective,
        codex_home: PathBuf,
        launcher: Arc<dyn Launcher>,
        timer: T,
    ) -> ProcessReaders<T> {
        let claude = effective.agent(CLAUDE_ID).cloned();
        if let Some(launch) = &claude
            && let Err(err) = paths::claude(launch)
        {
            tracing::warn!(error = %err, "Claude's limits reader is refused");
        }
        ProcessReaders {
            launcher,
            timer,
            claude,
            run_dir: effective.data_dir().join("limits").join("run"),
            codex_home,
        }
    }
}

impl<T: Timer + Clone + Send + Sync + 'static> ReadLimits for ProcessReaders<T> {
    fn read(&self, agent: AgentKind) -> Pin<Box<dyn Future<Output = ReadOutcome> + Send>> {
        match agent {
            AgentKind::Claude => {
                let Some(launch) = self.claude.clone() else {
                    let message = format!("no settings entry for {CLAUDE_ID}");
                    return Box::pin(ready(ReadOutcome::Failed(ReadFailure::LaunchFailed(
                        message,
                    ))));
                };
                let launcher = Arc::clone(&self.launcher);
                let timer = self.timer.clone();
                let run_dir = self.run_dir.clone();
                Box::pin(async move { claude::read(&*launcher, &timer, &launch, &run_dir).await })
            }
            // Replaced by the Codex reader after R14.
            AgentKind::Codex => {
                tracing::debug!(
                    codex_home = %self.codex_home.display(),
                    "the Codex reader is not built yet"
                );
                Box::pin(ready(ReadOutcome::Failed(ReadFailure::LaunchFailed(
                    "the Codex reader is not built yet".to_owned(),
                ))))
            }
        }
    }
}
