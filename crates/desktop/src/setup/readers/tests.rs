//! The swappable readers over a launcher that records each spec and starts nothing: Claude's
//! read gets Clauseo's own home as `CLAUDE_CONFIG_DIR` over an inherited one and the stored key
//! from the key slot, an entry set after a hold reads only once the hold drops, and with no
//! entry there is no launch. A refused entry's warning names no path (review L2). An agent an
//! unreadable policy blocks never reads, its stored key going nowhere (the final review's M2).

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use clauseo_agents::keys::Keychain;
use clauseo_limits::{ReadFailure, ReadLimits, ReadOutcome};
use clauseo_process::{Error, LaunchSpec, Launched, Launcher};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::{AgentLaunch, Effective, Locations, Plan, PlanProvider, SystemFiles};
use futures::FutureExt as _;

use super::{LeaveReaders, SwappableReaders, read_vars};
use crate::bridge::lock;
use crate::setup::keys::KeyCache;
use crate::test_support::{Scratch, block_on};

/// Records each spec and refuses to start it.
#[derive(Default)]
struct Recording(Mutex<Vec<LaunchSpec>>);

impl Launcher for Recording {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, Error> {
        lock(&self.0).push(spec.clone());
        Err(Error::InvalidSpec("the test starts nothing".to_owned()))
    }
}

const SCRIPT: &str = "install/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js";
const SDK: &str = "install/node_modules/@anthropic-ai/claude-agent-sdk/package.json";
const KEY: &str = "fakeAnthropicKey0123";

#[expect(
    clippy::disallowed_methods,
    reason = "test set-up writes a fake install; the desktop crate has no write module"
)]
fn fake_file(root: &Path, rel: &str, text: &str) -> String {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

#[test]
fn claude_s_read_has_the_home_and_the_key_and_a_hold_survives_an_entry_set() {
    let scratch = Scratch::folder("setup-readers", &["data"]);
    let script = fake_file(scratch.path(), SCRIPT, "");
    fake_file(
        scratch.path(),
        SDK,
        r#"{"name":"@anthropic-ai/claude-agent-sdk"}"#,
    );
    let locations = Locations::under_root(&scratch.path().join("data"));
    let effective = Effective::load(&locations, &SystemFiles, &[]);
    let claude = AgentId::new("claude-acp").unwrap();
    let keychain = Keychain::memory();
    keychain.store(&claude, KEY).unwrap();
    let slot = Arc::new(OnceLock::new());
    let _set = slot.set(Arc::new(KeyCache::load_all(keychain, [&claude])));
    let launcher = Arc::new(Recording::default());
    let readers = SwappableReaders::new(
        &effective,
        Arc::clone(&launcher) as Arc<dyn Launcher>,
        slot,
        Arc::default(),
        Arc::default(),
    );
    // No entry yet: no launch.
    assert!(matches!(
        block_on(readers.read(AgentKind::Claude)),
        ReadOutcome::Failed(ReadFailure::LaunchFailed(_))
    ));
    assert!(lock(&launcher.0).is_empty());
    let hold = block_on(LeaveReaders::hold(&readers, AgentKind::Claude));
    let entry = AgentLaunch {
        command: if cfg!(windows) {
            r"C:\node\node.exe".to_owned()
        } else {
            "/usr/local/bin/node".to_owned()
        },
        args: vec![script],
        env: [("CLAUDE_CONFIG_DIR".to_owned(), "/elsewhere".to_owned())].into(),
    };
    readers.set_entry(AgentKind::Claude, Some(entry));
    let mut read = readers.read(AgentKind::Claude);
    assert!(
        (&mut read).now_or_never().is_none(),
        "held across set_entry"
    );
    assert!(lock(&launcher.0).is_empty());
    drop(hold);
    assert!(matches!(block_on(read), ReadOutcome::Failed(_)));
    let specs = lock(&launcher.0).clone();
    let [spec] = specs.as_slice() else {
        panic!("one launch, got {}", specs.len());
    };
    let home = locations.agent_home("claude-acp").into_os_string();
    assert_eq!(spec.env().get(OsStr::new("CLAUDE_CONFIG_DIR")), Some(&home));
    assert!(
        spec.env().get(OsStr::new("ANTHROPIC_API_KEY")) == Some(&OsString::from(KEY)),
        "the stored key is not in the read's env"
    );
}

/// Reads Claude's global config in `home` as each read starts, and starts nothing.
struct SeesConfig {
    home: std::path::PathBuf,
    seen: Mutex<Vec<Option<Vec<u8>>>>,
}

impl Launcher for SeesConfig {
    fn launch(&self, _spec: &LaunchSpec) -> Result<Launched, Error> {
        lock(&self.seen).push(std::fs::read(self.home.join(".claude.json")).ok());
        Err(Error::InvalidSpec("the test starts nothing".to_owned()))
    }
}

/// Each Claude read prepares Claude's home first, as each launch does: an `env` planted in its
/// `.claude.json` is gone when the read starts Claude Code, and its state stays.
#[test]
fn a_claude_read_starts_after_the_homes_planted_settings_are_gone() {
    let scratch = Scratch::folder("setup-readers-home", &["data"]);
    let script = fake_file(scratch.path(), SCRIPT, "");
    fake_file(
        scratch.path(),
        SDK,
        r#"{"name":"@anthropic-ai/claude-agent-sdk"}"#,
    );
    let locations = Locations::under_root(&scratch.path().join("data"));
    let home = locations.agent_home("claude-acp");
    let config = home.join(".claude.json");
    let planted = r#"{"env": {"ANTHROPIC_BASE_URL": "https://example.invalid"}, "userID": "u"}"#;
    let relative = config.strip_prefix(scratch.path()).unwrap();
    fake_file(scratch.path(), relative.to_str().unwrap(), planted);
    let effective = Effective::load(&locations, &SystemFiles, &[]);
    let claude = AgentId::new("claude-acp").unwrap();
    let keychain = Keychain::memory();
    keychain.store(&claude, KEY).unwrap();
    let slot = Arc::new(OnceLock::new());
    let _set = slot.set(Arc::new(KeyCache::load_all(keychain, [&claude])));
    let launcher = Arc::new(SeesConfig {
        home,
        seen: Mutex::default(),
    });
    let readers = SwappableReaders::new(
        &effective,
        Arc::clone(&launcher) as Arc<dyn Launcher>,
        slot,
        Arc::default(),
        Arc::default(),
    );
    let entry = AgentLaunch {
        command: if cfg!(windows) {
            r"C:\node\node.exe".to_owned()
        } else {
            // unix-paths-ok: the cfg!(windows) branch above uses a Windows path there
            "/usr/local/bin/node".to_owned()
        },
        args: vec![script],
        env: std::collections::BTreeMap::new(),
    };
    readers.set_entry(AgentKind::Claude, Some(entry));
    assert!(matches!(
        block_on(readers.read(AgentKind::Claude)),
        ReadOutcome::Failed(_)
    ));
    let seen = lock(&launcher.seen).clone();
    let [Some(bytes)] = seen.as_slice() else {
        panic!("one launch with the config there, got {seen:?}");
    };
    let config: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(config, serde_json::json!({"userID": "u"}));
}

/// The security review's M4: a read carries exactly the variables the launch hook adds for the
/// plan and the key (one function makes both), so with a firm endpoint and a stored key the
/// read goes to the firm's gateway with the key.
#[test]
fn a_read_s_variables_are_the_launch_hook_s_for_the_plan_and_the_key() {
    use clauseo_host::ProfileHook as _;

    use crate::setup::hook::SetupHook;

    let scratch = Scratch::folder("setup-readers-plan", &["data"]);
    let script = fake_file(scratch.path(), SCRIPT, "");
    fake_file(
        scratch.path(),
        SDK,
        r#"{"name":"@anthropic-ai/claude-agent-sdk"}"#,
    );
    let locations = Locations::under_root(&scratch.path().join("data"));
    let effective = Effective::load(&locations, &SystemFiles, &[]);
    let claude = AgentId::new("claude-acp").unwrap();
    let keychain = Keychain::memory();
    keychain.store(&claude, KEY).unwrap();
    let slot = Arc::new(OnceLock::new());
    let _set = slot.set(Arc::new(KeyCache::load_all(keychain, [&claude])));
    let plan = Plan {
        endpoint: Some("https://gw.firm.example".to_owned()),
        provider: Some(PlanProvider::Bedrock),
        env: [("AWS_REGION".to_owned(), "eu-west-2".to_owned())].into(),
    };
    let plans: crate::setup::hook::Plans =
        Arc::new(std::sync::RwLock::new([(claude.clone(), plan)].into()));
    let network: crate::setup::NetworkSlot = Arc::default();
    let hook = SetupHook::new(Arc::clone(&slot), Arc::clone(&plans), Arc::clone(&network));
    let hooked: Vec<(String, String)> = hook
        .launch_env(&claude)
        .into_iter()
        .map(|(name, value)| (name.into_string().unwrap(), value.into_string().unwrap()))
        .collect();
    let read: Vec<(String, String)> = read_vars(&slot, &plans, &network, AgentKind::Claude)
        .into_iter()
        .map(|(name, value)| (name, String::clone(&value)))
        .collect();
    assert_eq!(read, hooked);
    let names: Vec<&str> = hooked.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "AWS_REGION",
            "CLAUDE_CODE_USE_BEDROCK",
            "NODE_USE_SYSTEM_CA"
        ]
    );
    // And a read's spec carries each of them.
    let launcher = Arc::new(Recording::default());
    let readers = SwappableReaders::new(
        &effective,
        Arc::clone(&launcher) as Arc<dyn Launcher>,
        slot,
        plans,
        Arc::default(),
    );
    let entry = AgentLaunch {
        command: if cfg!(windows) {
            r"C:\node\node.exe".to_owned()
        } else {
            "/usr/local/bin/node".to_owned()
        },
        args: vec![script],
        env: [("ANTHROPIC_API_KEY".to_owned(), String::new())].into(),
    };
    readers.set_entry(AgentKind::Claude, Some(entry));
    assert!(matches!(
        block_on(readers.read(AgentKind::Claude)),
        ReadOutcome::Failed(_)
    ));
    let specs = lock(&launcher.0).clone();
    let [spec] = specs.as_slice() else {
        panic!("one launch, got {}", specs.len());
    };
    for (name, value) in &hooked {
        assert!(
            spec.env().get(OsStr::new(name)) == Some(&OsString::from(value)),
            "{name} is not the hook's in the read's env"
        );
    }
}

/// A log line sink, shared with the subscriber.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        lock(&self.0).extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Decision record 0047 keeps paths out of `host.log`: an entry whose script has no SDK beside it
/// is refused, and the warning says so by the agent and the kind alone.
#[test]
fn a_refused_entry_s_warning_names_no_path() {
    let scratch = Scratch::folder("setup-readers-refused", &["data"]);
    let script = fake_file(scratch.path(), SCRIPT, "");
    let locations = Locations::under_root(&scratch.path().join("data"));
    let effective = Effective::load(&locations, &SystemFiles, &[]);
    let readers = SwappableReaders::new(
        &effective,
        Arc::new(Recording::default()) as Arc<dyn Launcher>,
        Arc::new(OnceLock::new()),
        Arc::default(),
        Arc::default(),
    );
    let entry = AgentLaunch {
        command: if cfg!(windows) {
            r"C:\node\node.exe".to_owned()
        } else {
            "/usr/local/bin/node".to_owned()
        },
        args: vec![script],
        env: std::collections::BTreeMap::new(),
    };
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        readers.set_entry(AgentKind::Claude, Some(entry));
    });
    let text = String::from_utf8(lock(&lines.0).clone()).unwrap();
    assert!(text.contains("a limits reader is refused"), "{text}");
    assert!(text.contains("kind=") && text.contains("refused"), "{text}");
    let root = scratch.path().display().to_string();
    assert!(!text.contains(&root), "a path reached the log: {text}");
    assert!(
        !text.contains("node_modules"),
        "a path reached the log: {text}"
    );
}

/// R20 P5b A4, c1: before the keys load, a read of an agent whose key is stored waits and
/// launches nothing, so it can show no other login's numbers; once they load with that key
/// unreadable, it fails without a launch.
#[test]
fn before_the_keys_load_a_stored_key_agent_s_read_shows_no_account_s_numbers() {
    let scratch = Scratch::folder("setup-readers-before-keys", &["data"]);
    let script = fake_file(scratch.path(), SCRIPT, "");
    fake_file(
        scratch.path(),
        SDK,
        r#"{"name":"@anthropic-ai/claude-agent-sdk"}"#,
    );
    let locations = Locations::under_root(&scratch.path().join("data"));
    let effective = Effective::load(&locations, &SystemFiles, &[]);
    let claude = AgentId::new("claude-acp").unwrap();
    let keychain = Keychain::memory();
    keychain.store(&claude, KEY).unwrap();
    let slot: crate::setup::KeySlot = Arc::new(OnceLock::new());
    let launcher = Arc::new(Recording::default());
    let readers = SwappableReaders::new(
        &effective,
        Arc::clone(&launcher) as Arc<dyn Launcher>,
        Arc::clone(&slot),
        Arc::default(),
        Arc::default(),
    );
    let entry = AgentLaunch {
        command: if cfg!(windows) {
            r"C:\node\node.exe".to_owned()
        } else {
            "/usr/local/bin/node".to_owned()
        },
        args: vec![script],
        env: std::collections::BTreeMap::new(),
    };
    readers.set_entry(AgentKind::Claude, Some(entry));
    let mut read = readers.read(AgentKind::Claude);
    assert!(
        (&mut read).now_or_never().is_none(),
        "the read waits for the keys"
    );
    assert!(
        lock(&launcher.0).is_empty(),
        "nothing read before the keys load"
    );
    // The keys load with Claude's stored key unreadable: the read fails, launching nothing.
    keychain.fail_reads(true);
    let _set = slot.set(Arc::new(KeyCache::load_all(keychain, [&claude])));
    assert!(matches!(
        block_on(read),
        ReadOutcome::Failed(ReadFailure::LaunchFailed(_))
    ));
    assert!(lock(&launcher.0).is_empty(), "no read with another login");
}

/// The final security review's M2: an agent an unreadable policy blocks
/// (`signin.policy_unreadable`) with a stored key never reads, so its key goes nowhere; its
/// reads go out once the block lifts.
#[test]
fn a_blocked_agent_with_a_stored_key_never_reads() {
    let scratch = Scratch::folder("setup-readers-blocked", &["data"]);
    let script = fake_file(scratch.path(), SCRIPT, "");
    fake_file(
        scratch.path(),
        SDK,
        r#"{"name":"@anthropic-ai/claude-agent-sdk"}"#,
    );
    let locations = Locations::under_root(&scratch.path().join("data"));
    let effective = Effective::load(&locations, &SystemFiles, &[]);
    let claude = AgentId::new("claude-acp").unwrap();
    let keychain = Keychain::memory();
    keychain.store(&claude, KEY).unwrap();
    let slot = Arc::new(OnceLock::new());
    let _set = slot.set(Arc::new(KeyCache::load_all(keychain, [&claude])));
    let launcher = Arc::new(Recording::default());
    let blocked = std::collections::BTreeSet::from([claude]);
    let readers = SwappableReaders::new(
        &effective,
        Arc::clone(&launcher) as Arc<dyn Launcher>,
        slot,
        Arc::default(),
        Arc::default(),
    )
    .with_blocked(blocked.clone());
    let entry = AgentLaunch {
        command: if cfg!(windows) {
            r"C:\node\node.exe".to_owned()
        } else {
            "/usr/local/bin/node".to_owned()
        },
        args: vec![script],
        env: std::collections::BTreeMap::new(),
    };
    readers.set_entry(AgentKind::Claude, Some(entry));
    let ReadOutcome::Failed(ReadFailure::LaunchFailed(why)) =
        block_on(readers.read(AgentKind::Claude))
    else {
        panic!("a blocked read must fail without a launch");
    };
    assert!(!why.contains(KEY), "{why}");
    assert!(lock(&launcher.0).is_empty(), "the key went nowhere");
    // The block lifts: the read goes out, with the key.
    readers.set_blocked(std::collections::BTreeSet::new());
    block_on(readers.read(AgentKind::Claude));
    assert_eq!(lock(&launcher.0).len(), 1, "read once the block lifted");
    // Blocked again (a `SettingsChanged`): no further read.
    readers.set_blocked(blocked);
    assert!(matches!(
        block_on(readers.read(AgentKind::Claude)),
        ReadOutcome::Failed(ReadFailure::LaunchFailed(_))
    ));
    assert_eq!(lock(&launcher.0).len(), 1, "nothing read while blocked");
}

/// The login keychain locked: Claude's read fails at once with no launch (its Claude Code would
/// ask the keychain and show the unlock dialog), so the last numbers stay as stale; Codex's
/// read is not held; unlocked again, Claude's next read goes out.
#[test]
fn a_locked_keychain_holds_claude_s_read_and_the_next_read_goes_out() {
    let scratch = Scratch::folder("setup-readers-locked", &["data"]);
    let script = fake_file(scratch.path(), SCRIPT, "");
    fake_file(
        scratch.path(),
        SDK,
        r#"{"name":"@anthropic-ai/claude-agent-sdk"}"#,
    );
    let locations = Locations::under_root(&scratch.path().join("data"));
    let effective = Effective::load(&locations, &SystemFiles, &[]);
    let slot = Arc::new(OnceLock::new());
    let _set = slot.set(Arc::new(KeyCache::load_all(
        Keychain::memory(),
        std::iter::empty(),
    )));
    let launcher = Arc::new(Recording::default());
    let locked = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let seen = Arc::clone(&locked);
    let readers = SwappableReaders::new(
        &effective,
        Arc::clone(&launcher) as Arc<dyn Launcher>,
        slot,
        Arc::default(),
        Arc::default(),
    )
    .with_keychain_locked(move || seen.load(std::sync::atomic::Ordering::SeqCst));
    let entry = AgentLaunch {
        command: if cfg!(windows) {
            r"C:\node\node.exe".to_owned()
        } else {
            // unix-paths-ok: the cfg!(windows) branch above uses a Windows path there
            "/usr/local/bin/node".to_owned()
        },
        args: vec![script],
        env: std::collections::BTreeMap::new(),
    };
    readers.set_entry(AgentKind::Claude, Some(entry));
    let ReadOutcome::Failed(ReadFailure::LaunchFailed(why)) =
        block_on(readers.read(AgentKind::Claude))
    else {
        panic!("a read with the keychain locked must fail without a launch");
    };
    assert_eq!(why, "the login keychain is locked");
    assert!(
        lock(&launcher.0).is_empty(),
        "a Claude read went out while locked"
    );
    locked.store(false, std::sync::atomic::Ordering::SeqCst);
    block_on(readers.read(AgentKind::Claude));
    assert_eq!(
        lock(&launcher.0).len(),
        1,
        "the read after the unlock did not go out"
    );
}

/// Whether a Claude read gets past the key check with a keychain that fails every read and no
/// "key stored" mark, under `plan` (decision record 0048: a failed read refuses under a firm
/// plan whatever the marker; with none, the read goes out keyless).
fn keys_ready_with_failed_read(plan: Option<Plan>) -> Result<(), ReadFailure> {
    let scratch = Scratch::folder("readers-firm-failed-read", &[]);
    let claude = AgentId::new("claude-acp").unwrap();
    let keychain = Keychain::memory();
    keychain.fail_reads(true);
    let notes = crate::setup::keys::KeyNotes::new();
    let cache = KeyCache::load_bounded(
        keychain.clone(),
        [&claude],
        crate::setup::keys::KeyMarker::new(scratch.path(), [&claude]),
        notes.sender(),
        &clauseo_process::AsyncIoTimer,
        crate::setup::keys::KEYS_LOAD_BOUND,
    );
    assert!(!cache.unreadable(&claude) && cache.read_failed(&claude));
    let slot = Arc::new(OnceLock::new());
    let _set = slot.set(Arc::new(cache));
    let plans: crate::setup::hook::Plans = Arc::new(std::sync::RwLock::new(
        plan.map(|plan| (claude.clone(), plan))
            .into_iter()
            .collect(),
    ));
    let ready = block_on(super::keys_ready(&slot, &plans, AgentKind::Claude));
    keychain.fail_reads(false);
    ready
}

#[test]
fn a_failed_key_read_under_a_firm_endpoint_or_provider_fails_the_read() {
    let endpoint = Plan {
        endpoint: Some("https://gw.firm.example".to_owned()),
        ..Plan::default()
    };
    assert!(keys_ready_with_failed_read(Some(endpoint)).is_err());
    for provider in [PlanProvider::Bedrock, PlanProvider::Vertex] {
        let plan = Plan {
            provider: Some(provider),
            ..Plan::default()
        };
        assert!(
            keys_ready_with_failed_read(Some(plan)).is_err(),
            "{provider:?}"
        );
    }
}

#[test]
fn a_failed_key_read_with_no_firm_plan_lets_the_read_go_out() {
    assert!(keys_ready_with_failed_read(None).is_ok());
    let env_only = Plan {
        env: [("AWS_REGION".to_owned(), "eu-west-2".to_owned())].into(),
        ..Plan::default()
    };
    assert!(keys_ready_with_failed_read(Some(env_only)).is_ok());
}
