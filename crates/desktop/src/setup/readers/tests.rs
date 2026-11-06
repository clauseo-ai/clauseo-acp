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
use clauseo_settings::{AgentLaunch, Effective, Locations, SystemFiles};
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

/// The security review's M4: a read carries exactly the variables the launch hook adds for the
/// plan and the key (one function makes both), so with a firm endpoint and a stored key the
/// read goes to the firm's gateway with the key.
#[test]
fn a_read_s_variables_are_the_launch_hook_s_for_the_plan_and_the_key() {
    use clauseo_host::ProfileHook as _;
    use clauseo_settings::{Plan, PlanProvider};

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
    let hook = SetupHook::new(Arc::clone(&slot), Arc::clone(&plans));
    let hooked: Vec<(String, String)> = hook
        .launch_env(&claude)
        .into_iter()
        .map(|(name, value)| (name.into_string().unwrap(), value.into_string().unwrap()))
        .collect();
    let read: Vec<(String, String)> = read_vars(&slot, &plans, AgentKind::Claude)
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
            "CLAUDE_CODE_USE_BEDROCK"
        ]
    );
    // And a read's spec carries each of them.
    let launcher = Arc::new(Recording::default());
    let readers = SwappableReaders::new(
        &effective,
        Arc::clone(&launcher) as Arc<dyn Launcher>,
        slot,
        plans,
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
