//! One live read: the variables, the temp data folder and its settings, the read through
//! `ProcessReaders`, the recording, and the check against what S13's types expect.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use clauseo_limits::{ProcessReaders, ReadFailure, ReadLimits, ReadOutcome, StdoutTap};
use clauseo_process::{AsyncIoTimer, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{AccountLimits, AgentKind, Billing, BillingProvider, LimitSource};
use clauseo_settings::{Effective, Locations, SystemFiles};
use futures::executor::block_on;

use crate::check::check_dir;
use crate::redact::{self, Redactor};
use crate::write;

/// The variable naming the folder `npm ci` filled from the pins file.
const INSTALL: &str = "CLAUSEO_G16_INSTALL";
/// The variable naming the absolute path of `node`.
const NODE: &str = "CLAUSEO_G16_NODE";
/// claude-agent-acp's bin script under the install.
const CLAUDE_SCRIPT: &str = "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js";
/// codex-acp's bin script under the install.
const CODEX_SCRIPT: &str = "node_modules/@agentclientprotocol/codex-acp/dist/index.js";

/// A login kind, one per test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Claude, a Max plan.
    ClaudeMax,
    /// Claude, a Team plan.
    ClaudeTeam,
    /// Claude, an Anthropic API key.
    ClaudeApi,
    /// Codex, a `ChatGPT` Plus plan.
    CodexPlus,
    /// Codex, an `OpenAI` API key.
    CodexApi,
}

impl Kind {
    /// The test's name, which names its recording.
    fn name(self) -> &'static str {
        match self {
            Kind::ClaudeMax => "claude_max",
            Kind::ClaudeTeam => "claude_team",
            Kind::ClaudeApi => "claude_api",
            Kind::CodexPlus => "codex_plus",
            Kind::CodexApi => "codex_api",
        }
    }

    /// The variable naming its login folder.
    fn var(self) -> String {
        format!("CLAUSEO_G16_{}", self.name().to_ascii_uppercase())
    }

    fn agent(self) -> AgentKind {
        match self {
            Kind::ClaudeMax | Kind::ClaudeTeam | Kind::ClaudeApi => AgentKind::Claude,
            Kind::CodexPlus | Kind::CodexApi => AgentKind::Codex,
        }
    }

    /// The pay-as-you-go provider of an API-key kind; `None` for a plan.
    fn pay_as_you_go(self) -> Option<BillingProvider> {
        match self {
            Kind::ClaudeApi => Some(BillingProvider::Anthropic),
            Kind::CodexApi => Some(BillingProvider::OpenAi),
            Kind::ClaudeMax | Kind::ClaudeTeam | Kind::CodexPlus => None,
        }
    }
}

/// Runs `kind`'s live read, or prints its skip line when its login variable is unset or empty.
pub(crate) fn live(kind: Kind) {
    let var = kind.var();
    let Some(login) = std::env::var_os(&var).filter(|value| !value.is_empty()) else {
        println!("skipped: no logins ({var})");
        return;
    };
    let install = required(INSTALL);
    let node = required(NODE);
    let scratch = Scratch::new(kind.name());
    let data = scratch.0.join("data");
    write::create_dir_all(&data).unwrap();
    let settings = settings(kind, &install, &node, &login);
    write::write(&data.join("settings.json"), settings.to_string()).unwrap();
    let effective = Effective::load(&Locations::under_root(&data), &SystemFiles, &[]);
    // Claude's reader ignores the home; Codex's reads its login from it.
    let codex_home = match kind.agent() {
        AgentKind::Codex => PathBuf::from(&login),
        AgentKind::Claude => scratch.0.join("codex-home"),
    };
    let raw = Arc::new(Mutex::new(Vec::<u8>::new()));
    let copy = Arc::clone(&raw);
    let tap: StdoutTap = Arc::new(move |_agent, chunk| {
        copy.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(chunk);
    });
    let launcher: Arc<dyn Launcher> = Arc::new(PassthroughLauncher::new(SpawnOptions::new()));
    let readers =
        ProcessReaders::new(&effective, codex_home, launcher, AsyncIoTimer).with_stdout_tap(tap);
    let outcome = block_on(readers.read(kind.agent()));
    let raw = raw.lock().unwrap_or_else(PoisonError::into_inner).clone();
    record(kind, &raw);
    expect(kind, &outcome);
}

/// The value of `var`, which a live run needs once its login variable is set.
fn required(var: &str) -> OsString {
    std::env::var_os(var)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("{var} is unset; a live run needs it (G16's runbook)"))
}

/// `settings.json`: the agent's entry running its bin script under `install` with `node`.
fn settings(
    kind: Kind,
    install: &OsString,
    node: &OsString,
    login: &OsString,
) -> serde_json::Value {
    let under = |rel: &str| {
        rel.split('/')
            .fold(PathBuf::from(install), |p, part| p.join(part))
    };
    let node = Path::new(node);
    let entry = match kind.agent() {
        AgentKind::Claude => serde_json::json!({
            "command": node,
            "args": [under(CLAUDE_SCRIPT)],
            "env": {"CLAUDE_CONFIG_DIR": Path::new(login)},
        }),
        AgentKind::Codex => serde_json::json!({
            "command": node,
            "args": [under(CODEX_SCRIPT)],
        }),
    };
    let id = match kind.agent() {
        AgentKind::Claude => "claude-acp",
        AgentKind::Codex => "codex-acp",
    };
    serde_json::json!({ "agents": { id: entry } })
}

/// Writes `raw`, redacted line by line, to `<target>/g16/<kind>.jsonl`, and prints the secret
/// check's findings over that folder.
fn record(kind: Kind, raw: &[u8]) {
    let mut redactor = Redactor::default();
    let text = String::from_utf8_lossy(raw);
    let mut lines = String::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        lines.push_str(&redactor.line(line));
        lines.push('\n');
    }
    let dir = target_dir().join("g16");
    write::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.jsonl", kind.name()));
    write::write(&path, lines).unwrap();
    println!("{}: recorded {}", kind.name(), path.display());
    for finding in check_dir(&dir).findings {
        println!("{}: the secret check fails: {finding}", kind.name());
    }
}

/// `CARGO_TARGET_DIR`, else the workspace's `target`.
fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .filter(|value| !value.is_empty())
        .map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            PathBuf::from,
        )
}

/// Checks `outcome` against what S13's types expect for `kind`: a plan's windows, each 0 to 100,
/// or pay as you go with no windows and `NotApplicable`. A mismatch names the shape it got,
/// never the account.
fn expect(kind: Kind, outcome: &ReadOutcome) {
    let name = kind.name();
    let limits = match outcome {
        ReadOutcome::Usage { limits, .. } => limits,
        other => panic!("{name}: expected usage, got {}", describe(other)),
    };
    let shape = shape(limits);
    if let Some(via) = kind.pay_as_you_go() {
        assert_eq!(
            limits.billing,
            Billing::PayAsYouGo { via },
            "{name}: {shape}"
        );
        assert_eq!(limits.source, LimitSource::NotApplicable, "{name}: {shape}");
        assert!(limits.windows.is_empty(), "{name}: {shape}");
    } else {
        assert_eq!(limits.billing, Billing::Plan, "{name}: {shape}");
        assert!(
            !matches!(
                limits.source,
                LimitSource::NotApplicable | LimitSource::Unavailable { .. }
            ),
            "{name}: {shape}"
        );
        assert!(!limits.windows.is_empty(), "{name}: no windows; {shape}");
        for window in &limits.windows {
            let used = window.used_percent.get();
            assert!(
                (0.0..=100.0).contains(&used),
                "{name}: {} at {used}",
                window.id
            );
        }
    }
    println!("{name}: ok, {shape}");
}

/// The outcome's kind, with a failure's redacted text; never an account.
fn describe(outcome: &ReadOutcome) -> String {
    match outcome {
        ReadOutcome::Usage { .. } => "usage".to_owned(),
        ReadOutcome::MethodMissing => "MethodMissing".to_owned(),
        ReadOutcome::SignedOut => "SignedOut".to_owned(),
        ReadOutcome::Failed(ReadFailure::Exited { code, stderr_tail }) => {
            format!("Exited {code:?}: {}", redact::text(stderr_tail))
        }
        ReadOutcome::Failed(failure) => format!("{failure:?}"),
    }
}

/// The limits' shape: billing, plan, source and windows, without the account.
fn shape(limits: &AccountLimits) -> String {
    let windows: Vec<String> = limits
        .windows
        .iter()
        .map(|window| format!("{} {}%", window.id, window.used_percent.get()))
        .collect();
    format!(
        "billing {:?}, plan {:?}, source {:?}, windows [{}]",
        limits.billing,
        limits.plan,
        limits.source,
        windows.join(", ")
    )
}

/// A temp folder of the test's own, removed at the end.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let root =
            std::env::temp_dir().join(format!("clauseo-limits-live-{}-{name}", std::process::id()));
        drop(write::remove_dir_all(&root));
        write::create_dir_all(&root).unwrap();
        Scratch(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}
