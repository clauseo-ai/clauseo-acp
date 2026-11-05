//! The readers: one read of an agent's plan usage through its vendor's own tool.
//!
//! `paths.rs` finds and checks the adapter's install folder, `env.rs` builds the environment,
//! `run.rs` runs a program under a deadline, `hold.rs` holds an agent's reads while its login
//! changes, `claude.rs` is Claude's reader and `codex.rs` Codex's.

mod claude;
mod codex;
mod env;
mod hold;
mod paths;
mod run;
mod tap;
mod time;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, PoisonError, RwLock};

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::{AccountLimits, AgentKind, LimitStopReason};
use clauseo_settings::{AgentLaunch, Effective};

pub(crate) use claude::looks_like_email;
pub use codex::CODEX_OVERRIDES;
pub use hold::HoldGuard;
use paths::{CLAUDE_ID, CODEX_ID};
#[cfg(feature = "live-tap")]
#[doc(hidden)]
pub use tap::StdoutTap;

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
        /// Whether the vendor's account answer named `limits.account`, or the reader went
        /// without it this time.
        account_read: AccountRead,
    },
    /// The usage method is missing or reshaped (Claude only): the source is unavailable, which
    /// is not an error.
    MethodMissing,
    /// The vendor's tool answered that no account is signed in (Codex's `account/read` with no
    /// account). The model shows the sign-in-again state for the agent, and stores nothing.
    SignedOut,
    /// The read failed.
    Failed(ReadFailure),
}

/// Whether a good read's account ref came from the vendor's account answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountRead {
    /// The account answer came, and the ref is what it said (an email, or the fixed ref when it
    /// had none).
    Read,
    /// The account answer did not come in time or failed, so the ref is the plan ref: the email
    /// was not read this time, which is not the same as an account with no email. The model
    /// keeps the last email it held for the same plan (`model/merge.rs`). Codex only.
    NotRead,
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

/// The variables a read of an agent adds over its entry's, asked at each read: the stored API
/// key and the firm plan's variables, as the caller's launch hook adds them.
type LaunchEnv = Arc<dyn Fn(AgentKind) -> Vec<(String, String)> + Send + Sync>;

/// The settings entries, shared by every copy of the readers ([`ProcessReaders::with_entry`]).
/// Never `Debug`-printed: an env may hold secrets.
#[derive(Default)]
struct Entries {
    claude: Option<AgentLaunch>,
    codex: Option<AgentLaunch>,
}

/// `agent`'s entry as it is now, with `launch_env`'s variables, each over any settings
/// variable of that name in any case; `None` with no entry. Asked once the read is past its
/// gate, so a read held over a login, key or entry change reads the new one.
fn entry_now(
    entries: &RwLock<Entries>,
    launch_env: Option<&LaunchEnv>,
    agent: AgentKind,
) -> Option<AgentLaunch> {
    let entries = entries.read().unwrap_or_else(PoisonError::into_inner);
    let mut launch = match agent {
        AgentKind::Claude => entries.claude.clone(),
        AgentKind::Codex => entries.codex.clone(),
    }?;
    drop(entries);
    for (name, value) in launch_env.map(|vars| vars(agent)).unwrap_or_default() {
        env::with_key(&mut launch.env, &name, value);
    }
    Some(launch)
}

/// The read of an agent with no entry.
fn no_entry(id: &str) -> ReadOutcome {
    ReadOutcome::Failed(ReadFailure::LaunchFailed(format!(
        "no settings entry for {id}"
    )))
}

/// Reads one agent's limits.
pub trait ReadLimits: Send + Sync {
    /// Reads `agent`'s limits once.
    fn read(&self, agent: AgentKind) -> Pin<Box<dyn Future<Output = ReadOutcome> + Send>>;
}

/// [`ReadLimits`] over a [`Launcher`] and a [`Timer`].
///
/// Each read checks the adapter's paths again and makes the working folder
/// `<data_dir>/limits/run` if it is missing (nothing in Clauseo writes there). Claude's read
/// writes the helper into its install folder when the file there differs and runs it; Codex's
/// runs `codex app-server` from codex-acp's install with `CODEX_HOME` forced to Clauseo's own
/// Codex home. Both run with the inherited environment under the agent's settings variables.
///
/// The read future does a little blocking file work before the launch, on its first poll: the
/// path checks, the compare of a ~4 KiB file, at most one small write, and the folder creation.
/// Poll it where a brief block is fine.
///
/// With [`ProcessReaders::with_launch_env`] each read also carries the caller's variables: the
/// agent's stored API key (`ANTHROPIC_API_KEY` for Claude, `CODEX_API_KEY` for Codex), so a key
/// sign-in reads as pay-as-you-go, and its firm plan's routing, so the read goes where the
/// agent's launches go. [`ProcessReaders::hold`] keeps an agent's reads off while its login
/// changes, until its [`HoldGuard`] drops (decision record 0048).
pub struct ProcessReaders<T> {
    launcher: Arc<dyn Launcher>,
    timer: T,
    /// Claude's and Codex's settings entries, shared by every copy, read once a read is past
    /// its gate.
    entries: Arc<RwLock<Entries>>,
    /// The readers' working folder, `<data_dir>/limits/run`.
    run_dir: PathBuf,
    /// Clauseo's own Codex home, which the Codex reader forces as `CODEX_HOME`.
    codex_home: PathBuf,
    /// Clauseo's own Claude home, which the Claude reader forces as `CLAUDE_CONFIG_DIR`;
    /// `None` leaves the entry's and the inherited variable as they are.
    claude_home: Option<PathBuf>,
    /// G16's copy of each reader's stdout; `None` everywhere but its live tests.
    tap: Option<tap::StdoutTap>,
    /// Each agent's stored key, asked at each read; `None` for no key source.
    launch_env: Option<LaunchEnv>,
    /// Claude's read gate.
    claude_gate: Arc<hold::Gate>,
    /// Codex's read gate.
    codex_gate: Arc<hold::Gate>,
}

impl<T: Timer + Clone + Send + Sync + 'static> ProcessReaders<T> {
    /// Readers for the agents `effective` has entries for. A Claude or Codex entry whose paths
    /// break a rule is logged once here, at `warn`, and each of its reads fails with the rule.
    ///
    /// `codex_home` is Clauseo's own Codex home, required and with no default. The Codex reader
    /// sets `CODEX_HOME` to it last, over any inherited or settings variable of that name in any
    /// case: codex-acp merges the user's `~/.codex/config.toml` (MCP servers, plugins, hooks,
    /// rules) unless `CODEX_HOME` is a separate folder, so Clauseo owns one, and Codex signs in
    /// separately inside Clauseo. Nothing here makes it or reads inside it: the sign-in (S12's,
    /// later) creates it, and the host (R16) passes it. While it is missing a Codex read is
    /// `SignedOut` without a launch (the app-server refuses a missing `CODEX_HOME`, and no login
    /// can be there); a link or a file there fails the read. The Claude reader ignores it.
    pub fn new(
        effective: &Effective,
        codex_home: PathBuf,
        launcher: Arc<dyn Launcher>,
        timer: T,
    ) -> ProcessReaders<T> {
        let claude = effective.agent(CLAUDE_ID).cloned();
        let codex = effective.agent(CODEX_ID).cloned();
        if let Some(launch) = &claude
            && let Err(err) = paths::claude(launch)
        {
            tracing::warn!(error = %err, "Claude's limits reader is refused");
        }
        if let Some(launch) = &codex
            && let Err(err) = paths::codex_js(launch)
        {
            tracing::warn!(error = %err, "Codex's limits reader is refused");
        }
        ProcessReaders {
            launcher,
            timer,
            entries: Arc::new(RwLock::new(Entries { claude, codex })),
            run_dir: effective.data_dir().join("limits").join("run"),
            codex_home,
            claude_home: None,
            tap: None,
            launch_env: None,
            claude_gate: Arc::default(),
            codex_gate: Arc::default(),
        }
    }

    /// These readers, asking `vars` at each read, once past the gate, for the variables the
    /// read adds: desktop gives the same ones its launch hook gives the agent (decision record
    /// 0048), the stored API key as `ANTHROPIC_API_KEY` (Claude) or `CODEX_API_KEY` (Codex) and
    /// the firm plan's routing (`ANTHROPIC_BASE_URL`, `CLAUDE_CODE_USE_BEDROCK`, the plan's
    /// `env`). Each goes over any settings variable of that name in any case; a name not given
    /// leaves the settings variable as it is. No value is logged, and a tail that echoes a key
    /// shows `[redacted]`.
    #[must_use]
    pub fn with_launch_env(
        mut self,
        vars: impl Fn(AgentKind) -> Vec<(String, String)> + Send + Sync + 'static,
    ) -> ProcessReaders<T> {
        self.launch_env = Some(Arc::new(vars));
        self
    }

    /// These readers, forcing `CLAUDE_CONFIG_DIR` to `claude_home` (Clauseo's own Claude home,
    /// `Locations::agent_home`) last in each Claude read's env, over any inherited or settings
    /// variable of that name in any case, as the host forces it at an agent's launch (decision
    /// record 0048): an inherited `CLAUDE_CONFIG_DIR` never points the read at another login.
    #[must_use]
    pub fn with_claude_home(mut self, claude_home: PathBuf) -> ProcessReaders<T> {
        self.claude_home = Some(claude_home);
        self
    }

    /// Replaces `agent`'s settings entry with `launch` (`None` for no entry), for desktop to
    /// call after each entry it writes. Every copy of these readers shares the entries, and a
    /// read takes its entry once past its gate, so a read queued behind a hold reads the entry
    /// set before the hold drops. An entry whose paths break a rule is logged here, at `warn`,
    /// as [`ProcessReaders::new`] logs one.
    pub fn set_entry(&self, agent: AgentKind, launch: Option<AgentLaunch>) {
        if let Some(launch) = &launch {
            let refused = match agent {
                AgentKind::Claude => paths::claude(launch).err(),
                AgentKind::Codex => paths::codex_js(launch).err(),
            };
            if let Some(err) = refused {
                tracing::warn!(error = %err, ?agent, "a limits reader is refused");
            }
        }
        let mut entries = self.entries.write().unwrap_or_else(PoisonError::into_inner);
        match agent {
            AgentKind::Claude => entries.claude = launch,
            AgentKind::Codex => entries.codex = launch,
        }
    }

    /// [`Self::set_entry`], then a copy of these readers. The copy shares these readers'
    /// entries, gates and variable source, so a hold taken on either holds both and a
    /// [`HoldGuard`] taken before survives the copy; every other part is kept.
    #[must_use]
    pub fn with_entry(&self, agent: AgentKind, launch: Option<AgentLaunch>) -> ProcessReaders<T> {
        self.set_entry(agent, launch);
        ProcessReaders {
            launcher: Arc::clone(&self.launcher),
            timer: self.timer.clone(),
            entries: Arc::clone(&self.entries),
            run_dir: self.run_dir.clone(),
            codex_home: self.codex_home.clone(),
            claude_home: self.claude_home.clone(),
            tap: self.tap.clone(),
            launch_env: self.launch_env.clone(),
            claude_gate: Arc::clone(&self.claude_gate),
            codex_gate: Arc::clone(&self.codex_gate),
        }
    }

    /// Holds `agent`'s reads from this call on, and answers a future that resolves to the
    /// hold's [`HoldGuard`] once none is running. A read asked for meanwhile waits, unlaunched,
    /// until every guard of that agent is dropped: two overlapping holds need two drops. The
    /// host keeps the guard around a sign-in, a sign-out and a key change, drops it, then sends
    /// `Input::LoginChanged`.
    pub fn hold(&self, agent: AgentKind) -> impl Future<Output = HoldGuard> + Send + 'static {
        self.gate(agent).hold()
    }

    fn gate(&self, agent: AgentKind) -> &Arc<hold::Gate> {
        match agent {
            AgentKind::Claude => &self.claude_gate,
            AgentKind::Codex => &self.codex_gate,
        }
    }

    /// These readers, copying each read's raw stdout to `tap`: G16's live tests record the
    /// vendors' answers through it (`tests/live.rs`). Not for the product: the bytes are
    /// unredacted, and the caller redacts them.
    #[doc(hidden)]
    #[must_use]
    #[cfg(feature = "live-tap")]
    pub fn with_stdout_tap(mut self, tap: StdoutTap) -> ProcessReaders<T> {
        self.tap = Some(tap);
        self
    }
}

impl<T: Timer + Clone + Send + Sync + 'static> ReadLimits for ProcessReaders<T> {
    fn read(&self, agent: AgentKind) -> Pin<Box<dyn Future<Output = ReadOutcome> + Send>> {
        match agent {
            AgentKind::Claude => {
                let entries = Arc::clone(&self.entries);
                let launch_env = self.launch_env.clone();
                let launcher = Arc::clone(&self.launcher);
                let timer = self.timer.clone();
                let run_dir = self.run_dir.clone();
                let claude_home = self.claude_home.clone();
                let tap = self.tap.clone();
                let gate = Arc::clone(&self.claude_gate);
                Box::pin(async move {
                    let _running = gate.enter().await;
                    let Some(launch) = entry_now(&entries, launch_env.as_ref(), agent) else {
                        return no_entry(CLAUDE_ID);
                    };
                    let home = claude_home.as_deref();
                    claude::read(&*launcher, &timer, &launch, &run_dir, home, tap).await
                })
            }
            AgentKind::Codex => {
                let entries = Arc::clone(&self.entries);
                let launch_env = self.launch_env.clone();
                let launcher = Arc::clone(&self.launcher);
                let timer = self.timer.clone();
                let run_dir = self.run_dir.clone();
                let codex_home = self.codex_home.clone();
                let tap = self.tap.clone();
                let gate = Arc::clone(&self.codex_gate);
                Box::pin(async move {
                    let _running = gate.enter().await;
                    let Some(launch) = entry_now(&entries, launch_env.as_ref(), agent) else {
                        return no_entry(CODEX_ID);
                    };
                    codex::read(&*launcher, &timer, &launch, &run_dir, &codex_home, tap).await
                })
            }
        }
    }
}
