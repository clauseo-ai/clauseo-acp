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

use crate::check::check_file;
use crate::redact::Redactor;
use crate::write;

/// The variable naming the folder `npm ci` filled from the pins file.
const INSTALL: &str = "CLAUSEO_G16_INSTALL";
/// The variable naming the absolute path of `node`.
const NODE: &str = "CLAUSEO_G16_NODE";
/// claude-agent-acp's bin script under the install.
const CLAUDE_SCRIPT: &str = "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js";
/// codex-acp's bin script under the install.
const CODEX_SCRIPT: &str = "node_modules/@agentclientprotocol/codex-acp/dist/index.js";
/// The key variables a plan test blanks in its settings entry, so a key the caller exported for
/// the whole run cannot turn a plan read into an API-key read. Claude Code and Codex 0.162.1
/// (`codex-rs/login/src/auth/manager.rs`, `read_non_empty_env_var`) both ignore an empty key.
const KEY_VARS: [&str; 3] = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "CODEX_API_KEY"];
/// The most raw stdout one read keeps; the rest is dropped and the run says so.
const RAW_MAX: usize = 4 * 1024 * 1024;

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

    /// A word the plan's name must hold, for a plan kind.
    fn plan_word(self) -> Option<&'static str> {
        match self {
            Kind::ClaudeMax => Some("Max"),
            Kind::ClaudeTeam => Some("Team"),
            Kind::CodexPlus => Some("Plus"),
            Kind::ClaudeApi | Kind::CodexApi => None,
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
    let raw = Arc::new(Mutex::new(Raw::default()));
    let copy = Arc::clone(&raw);
    let tap: StdoutTap = Arc::new(move |_agent, chunk| {
        copy.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keep(chunk);
    });
    let launcher: Arc<dyn Launcher> = Arc::new(PassthroughLauncher::new(SpawnOptions::new()));
    let readers = ProcessReaders::new(&effective, codex_home, launcher, AsyncIoTimer);
    let readers = readers.with_stdout_tap(tap);
    let outcome = block_on(readers.read(kind.agent()));
    let raw = std::mem::take(&mut *raw.lock().unwrap_or_else(PoisonError::into_inner));
    let mut redactor = Redactor::new(known(&[&login, &install, &node], &scratch.0));
    record(kind, &raw, &mut redactor);
    expect(kind, &outcome, &mut redactor);
}

/// One read's raw stdout, up to [`RAW_MAX`].
#[derive(Debug, Default)]
struct Raw {
    /// The bytes kept.
    bytes: Vec<u8>,
    /// Bytes past the cap were dropped.
    cut: bool,
}

impl Raw {
    /// Keeps `chunk`, up to the cap.
    fn keep(&mut self, chunk: &[u8]) {
        let room = RAW_MAX.saturating_sub(self.bytes.len());
        self.cut |= chunk.len() > room;
        self.bytes
            .extend_from_slice(chunk.get(..room.min(chunk.len())).unwrap_or_default());
    }
}

/// The run's own local values for the redactor: `values`, the temp folder (also under
/// macOS's `/private`, which the vendors report), the system temp folder and `$HOME`.
fn known(values: &[&OsString], scratch: &Path) -> Vec<String> {
    let mut known: Vec<String> = values
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    for path in [scratch.to_path_buf(), std::env::temp_dir()] {
        let path = path
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_owned();
        known.push(format!("/private{path}"));
        known.push(path);
    }
    if let Some(home) = std::env::var_os("HOME") {
        known.push(home.to_string_lossy().into_owned());
    }
    known
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
            "env": {},
        }),
    };
    let mut entry = entry;
    if kind.pay_as_you_go().is_none()
        && let Some(env) = entry
            .get_mut("env")
            .and_then(serde_json::Value::as_object_mut)
    {
        for var in KEY_VARS {
            env.insert(var.to_owned(), serde_json::Value::String(String::new()));
        }
    }
    let id = match kind.agent() {
        AgentKind::Claude => "claude-acp",
        AgentKind::Codex => "codex-acp",
    };
    serde_json::json!({ "agents": { id: entry } })
}

/// Writes `raw`, redacted line by line, to `<target>/g16/<kind>.jsonl`, then fails the test
/// when the secret check finds anything in that one file. A finding names the line and the rule,
/// never the value.
fn record(kind: Kind, raw: &Raw, redactor: &mut Redactor) {
    let text = String::from_utf8_lossy(&raw.bytes);
    let mut lines = String::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        lines.push_str(&redactor.line(line));
        lines.push('\n');
    }
    let dir = target_dir().join("g16");
    write::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.jsonl", kind.name()));
    write::write(&path, lines).unwrap();
    let cut = if raw.cut { " (cut at 4 MiB)" } else { "" };
    println!("{}: recorded {}{cut}", kind.name(), path.display());
    let findings = check_file(&path).findings;
    assert!(
        findings.is_empty(),
        "{}: the recording fails the secret check, so redaction missed something:\n{}",
        kind.name(),
        findings.join("\n")
    );
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

/// Checks `outcome` against what S13's types expect for `kind`: a plan whose name holds the
/// kind's word, with windows each 0 to 100 and a reset time on each one in use (S13's schema
/// keeps a null `resets_at`, as `fixtures/claude/pro.json` has, and the reader drops one that
/// is not RFC 3339); or pay as you go with no windows and `NotApplicable`. A mismatch names the
/// shape it got, never the account.
fn expect(kind: Kind, outcome: &ReadOutcome, redactor: &mut Redactor) {
    let name = kind.name();
    let limits = match outcome {
        ReadOutcome::Usage { limits, .. } => limits,
        other => panic!("{name}: expected usage, got {}", describe(other, redactor)),
    };
    let plan = limits.plan.as_ref().map(|plan| plan.0.as_str());
    println!("{name}: plan {plan:?}");
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
        let word = kind.plan_word().unwrap_or_default();
        assert!(
            plan.is_some_and(|plan| plan.contains(word)),
            "{name}: the plan is not {word}; {shape}"
        );
        assert!(!limits.windows.is_empty(), "{name}: no windows; {shape}");
        for window in &limits.windows {
            let used = window.used_percent.get();
            assert!(
                (0.0..=100.0).contains(&used),
                "{name}: {} at {used}",
                window.id
            );
            assert!(
                used <= 0.0 || window.resets_at.is_some(),
                "{name}: {} is in use with no reset time",
                window.id
            );
        }
    }
    println!("{name}: ok, {shape}");
}

/// The outcome's kind, with a failure's redacted text; never an account.
fn describe(outcome: &ReadOutcome, redactor: &mut Redactor) -> String {
    match outcome {
        ReadOutcome::Usage { .. } => "usage".to_owned(),
        ReadOutcome::MethodMissing => "MethodMissing".to_owned(),
        ReadOutcome::SignedOut => "SignedOut".to_owned(),
        ReadOutcome::Failed(ReadFailure::Exited { code, stderr_tail }) => {
            format!("Exited {code:?}: {}", redactor.text(stderr_tail))
        }
        ReadOutcome::Failed(failure) => redactor.text(&format!("{failure:?}")),
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
