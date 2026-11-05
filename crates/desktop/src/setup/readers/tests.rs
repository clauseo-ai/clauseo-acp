//! The swappable readers over a launcher that records each spec and starts nothing: Claude's
//! read gets Clauseo's own home as `CLAUDE_CONFIG_DIR` over an inherited one and the stored key
//! from the key slot, an entry set after a hold reads only once the hold drops, and with no
//! entry there is no launch.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use clauseo_agents::keys::Keychain;
use clauseo_limits::{ReadFailure, ReadLimits, ReadOutcome};
use clauseo_process::{Error, LaunchSpec, Launched, Launcher};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::{AgentLaunch, Effective, Locations, SystemFiles};
use futures::FutureExt as _;

use super::{LeaveReaders, SwappableReaders};
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
    let readers =
        SwappableReaders::new(&effective, Arc::clone(&launcher) as Arc<dyn Launcher>, slot);
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
