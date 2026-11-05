//! The launch prep's case-law relay against S28's fake connector: who gets one, the token file
//! (its mode, its content, gone with the run folder), two sessions apart, and a relay that
//! refuses once its session ends.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_connectors::fake::{FakeConnector, FakeHandle, FakeOptions, ScratchDir};
use clauseo_connectors::{
    ConnectorHandle, Connectors, CredentialStatus, McpRequestHeaders, MemoryStore, SecretStore,
};
use clauseo_protocol::{MatterId, ThreadId, Timestamp, ToolsUnavailableReason};

use super::super::{RunFolders, RunSweep};
use super::{CaseLaw, CaseLawPrep};
use crate::ManualClock;
use crate::agent::loads::browser::Switch;
use crate::config::SessionKey;
use crate::connectors::{FixedOffset, RelayConfig, RelayEvent, RelayEvents, Relays};
use crate::pipeline::tests::support::id;

struct NoEvents;

impl RelayEvents for NoEvents {
    fn event(&self, _event: RelayEvent) {}
}

/// The fake, its connector and the app's relays, with a data folder of its own.
struct Rig {
    fake: FakeHandle,
    scratch: ScratchDir,
    _connectors: Connectors,
    handle: ConnectorHandle,
    relays: Relays,
    /// One per host: a second one would set the first launch's folders aside.
    sweep: RunSweep,
}

fn rig() -> Rig {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let scratch = ScratchDir::new("launch-relay").unwrap();
    let config = fake.config(scratch.state_file());
    let origin = config.origin.clone();
    let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
    let connectors = Connectors::new(config, store).unwrap();
    let handle = connectors.get("case-law").unwrap();
    let relays = Relays::new(RelayConfig {
        connector: handle.clone(),
        origin,
        day_file: scratch.path().join("connectors").join("day.json"),
        clock: Arc::new(ManualClock::new(Timestamp::new(1_793_577_600_000).unwrap())),
        local_day: Arc::new(FixedOffset(19_800)),
        events: Arc::new(NoEvents),
    });
    Rig {
        fake,
        scratch,
        _connectors: connectors,
        handle,
        relays,
        sweep: RunSweep::default(),
    }
}

impl Rig {
    /// Adds an API key the fake accepts: the connector is signed in.
    fn sign_in(&self) {
        self.handle.add_api_key(&self.fake.new_api_key()).unwrap();
    }

    /// `launch`'s folders, made.
    fn run(&self, launch: u64) -> RunFolders {
        let data = self.data();
        crate::write::create_dir_all(&data).unwrap();
        let run = RunFolders::new(
            &data,
            SessionKey {
                matter: id::<MatterId>(7),
                thread: id::<ThreadId>(8),
                launch,
            },
        );
        run.make(&data, &self.sweep).unwrap();
        run
    }

    fn data(&self) -> PathBuf {
        self.scratch.path().join("data")
    }

    /// A prep with the relays, the switch `on` (`locked` by policy) and no managed MCP.
    fn prep(&self, on: bool, locked: bool) -> CaseLawPrep {
        CaseLawPrep {
            relays: Some(self.relays.clone()),
            switch: Switch { on, locked },
            managed_mcp: self.scratch.path().join("no-managed-mcp.json"),
        }
    }
}

/// The hello `clauseo-mcp connector` sends, and the relay's answer line.
fn hello(port: u16, token: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let line = format!(r#"{{"clauseo_relay":1,"token":"{token}","connector":"case-law"}}"#);
    stream.write_all(line.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).unwrap();
    answer.trim_end().to_owned()
}

const OK: &str = r#"{"clauseo_relay":1,"ok":true}"#;

fn token_in(file: &Path) -> String {
    std::fs::read_to_string(file).unwrap().trim_end().to_owned()
}

#[test]
fn a_signed_in_session_gets_a_relay_and_a_private_token_file_gone_with_its_folder() {
    let rig = rig();
    rig.sign_in();
    let run = rig.run(1);
    let case_law = rig.prep(true, false).open(&run);
    let CaseLaw::Open(relay) = &case_law else {
        panic!("no relay: {case_law:?}");
    };
    assert_eq!(run.relay_token, run.root.join("relay-token"));
    let token = token_in(&run.relay_token);
    assert_eq!(token.len(), 64);
    assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&run.relay_token)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
        let folder = std::fs::metadata(&run.root).unwrap().permissions().mode();
        assert_eq!(folder & 0o777, 0o700, "{folder:o}");
    }
    // The file's token is the relay's, and the agent's redactor gets it.
    assert_eq!(hello(relay.port(), &token), OK);
    let run = RunFolders { case_law, ..run };
    assert_eq!(super::redacted(&run), std::slice::from_ref(&token));
    // The session ends: the token is revoked before the folder, and its file, goes.
    super::end(&run);
    assert_eq!(
        hello(run.case_law.relay().unwrap().port(), &token),
        r#"{"clauseo_relay":1,"ok":false,"error":"closed"}"#
    );
    run.remove();
    assert!(!run.relay_token.exists());
    assert!(!run.root.exists());
}

#[test]
fn two_sessions_get_their_own_port_file_and_token() {
    let rig = rig();
    rig.sign_in();
    let (one, two) = (rig.run(1), rig.run(2));
    let (first, second) = (
        rig.prep(true, false).open(&one),
        rig.prep(true, false).open(&two),
    );
    let (Some(first), Some(second)) = (first.relay(), second.relay()) else {
        panic!("a session has no relay");
    };
    assert_ne!(first.port(), second.port());
    assert_ne!(one.relay_token, two.relay_token);
    let (token_one, token_two) = (token_in(&one.relay_token), token_in(&two.relay_token));
    assert_ne!(token_one, token_two);
    // Each listener takes only its own session's token.
    assert_eq!(hello(first.port(), &token_one), OK);
    assert_ne!(hello(first.port(), &token_two), OK);
    assert_ne!(hello(second.port(), &token_one), OK);
}

/// Every way to no relay: no file is written and nothing is bound (the prep returns before the
/// bind); the header state says why only once the connector is added.
#[test]
fn no_relay_and_no_token_file_unless_every_condition_holds() {
    let rig = rig();
    let run = rig.run(1);
    let check = |prep: CaseLawPrep, expected: Option<ToolsUnavailableReason>| {
        let case_law = prep.open(&run);
        match (&case_law, expected) {
            (CaseLaw::Absent, None) => {}
            (CaseLaw::Unavailable(reason), Some(expected)) => assert_eq!(*reason, expected),
            (other, _) => panic!("expected {expected:?}, got {other:?}"),
        }
        assert!(!run.relay_token.exists(), "a token file for {case_law:?}");
    };
    // No relays (none wired, or the hook offers no `case-law` entry).
    let mut none = rig.prep(true, false);
    none.relays = None;
    check(none, None);
    // Not added: nothing shows, whatever the switch.
    check(rig.prep(true, false), None);
    check(rig.prep(false, true), None);
    rig.sign_in();
    // The switch off: the lawyer's, or the firm's.
    check(rig.prep(false, false), Some(ToolsUnavailableReason::Off));
    check(
        rig.prep(false, true),
        Some(ToolsUnavailableReason::LockedOff),
    );
    // The firm's managed-mcp.json.
    let managed = rig.scratch.path().join("managed-mcp.json");
    crate::write::write_file(&managed, b"{}").unwrap();
    let mut prep = rig.prep(true, false);
    prep.managed_mcp = managed;
    check(prep, Some(ToolsUnavailableReason::ManagedByFirm));
    // Signed out: nothing in the header (the connector's own state says why).
    rig.handle.sign_out().unwrap();
    check(rig.prep(true, false), None);
}

#[test]
fn a_revoked_key_gives_no_relay() {
    let rig = rig();
    rig.sign_in();
    rig.fake.revoke_all();
    // The connector learns of a revoked key on a call; the handle's status then says so.
    let call =
        br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"read_document"}}"#;
    let _refused = rig.handle.mcp_post(call, &McpRequestHeaders::default());
    assert_eq!(rig.handle.status(), CredentialStatus::KeyRevoked);
    let run = rig.run(1);
    assert!(matches!(rig.prep(true, false).open(&run), CaseLaw::Absent));
    assert!(!run.relay_token.exists());
}

/// On Windows the relay's port is bound with `SO_EXCLUSIVEADDRUSE`: another socket cannot bind
/// it, not even with `SO_REUSEADDR`.
#[cfg(windows)]
#[test]
fn the_relay_port_cannot_be_bound_again_with_reuseaddr() {
    use socket2::{Domain, Protocol, Socket, Type};
    let rig = rig();
    rig.sign_in();
    let run = rig.run(1);
    let case_law = rig.prep(true, false).open(&run);
    let port = case_law.relay().unwrap().port();
    let address: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
    let other = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
    other.set_reuse_address(true).unwrap();
    assert!(
        other.bind(&address.into()).is_err(),
        "{address} bound twice"
    );
}

/// The firm's `managed-mcp.json` seen again right before the launch (`profile_loads`): the open
/// relay is ended, refuses its token, gives no entry and says why, and still names its token to
/// the redactor; it stops later with the run folder, off the actor.
#[test]
fn a_relay_ended_before_the_launch_refuses_and_says_why() {
    let rig = rig();
    rig.sign_in();
    let run = rig.run(1);
    let mut case_law = rig.prep(true, false).open(&run);
    let port = case_law.relay().unwrap().port();
    let token = token_in(&run.relay_token);
    case_law.end_before_launch(ToolsUnavailableReason::ManagedByFirm);
    assert!(case_law.relay().is_none());
    assert!(matches!(
        case_law,
        CaseLaw::Ended(_, ToolsUnavailableReason::ManagedByFirm)
    ));
    assert_eq!(
        hello(port, &token),
        r#"{"clauseo_relay":1,"ok":false,"error":"closed"}"#
    );
    let run = RunFolders { case_law, ..run };
    assert_eq!(super::redacted(&run), std::slice::from_ref(&token));
}
