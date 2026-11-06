//! The connector tests' rig (R24, PR5): S28's fake connector, the app's `Connectors` on a
//! `MemoryStore` keychain and on the host's own manual clock, the relays built with the host's
//! `ConnectorEvents`, an opener stand-in that hands the sign-in page to the test, and an app
//! subscription whose every event, and every reply the test gets, is kept as JSON for the leak
//! checks.

use std::net::TcpListener;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use clauseo_connectors::fake::{FakeConnector, FakeHandle, FakeOptions, ScratchDir};
use clauseo_connectors::{ConnectorHandle, Connectors, MemoryStore, SecretStore};
use clauseo_host::connectors::{
    ConnectorEvents, ConnectorOpener, ConnectorsWiring, FixedOffset, RelayConfig, RelaySession,
    Relays,
};
use clauseo_host::{AppSubscription, Clock as _, HostConfig, ManualClock, SessionKey};
use clauseo_protocol::{
    CommandOutcome, ConnectorCommand, ConnectorStatus, ConnectorView, ConnectorsState, HostCommand,
    HostError, HostEvent, MatterId, RedactedString, ThreadId,
};
use futures::StreamExt as _;

use crate::settings::decode;
use crate::support::{Dir, Running, foreign_uuid, wait};

/// The connector's id.
pub(crate) const CASE_LAW: &str = "case-law";

/// The connectors crate's clock, read from the host's manual clock, so both move together.
struct HostTime(ManualClock);

impl clauseo_connectors::Clock for HostTime {
    fn now(&self) -> SystemTime {
        let millis = u64::try_from(self.0.now().wall.as_millis()).unwrap();
        SystemTime::UNIX_EPOCH + Duration::from_millis(millis)
    }
}

/// The opener stand-in: hands each page to the test.
struct Pages(Mutex<mpsc::Sender<String>>);

impl ConnectorOpener for Pages {
    fn open(&self, url: &str) -> Result<(), String> {
        self.0.lock().unwrap().send(url.to_owned()).unwrap();
        Ok(())
    }
}

/// How the rig starts.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Start {
    /// Add an API key the fake accepts before the host starts.
    pub(crate) signed_in: bool,
}

/// The fake, the app's connectors, the relays and a running host wired with them.
pub(crate) struct Rig {
    pub(crate) fake: FakeHandle,
    pub(crate) handle: ConnectorHandle,
    pub(crate) relays: Relays,
    pub(crate) events: Arc<ConnectorEvents>,
    pub(crate) pages: mpsc::Receiver<String>,
    pub(crate) host: Running,
    pub(crate) sub: AppSubscription,
    /// Every app event and command reply so far, as JSON.
    pub(crate) seen: Vec<String>,
    /// The API keys the test made, for the leak checks.
    pub(crate) keys: Vec<String>,
    scratch_dir: ScratchDir,
}

impl Rig {
    /// The fake, then the host started in `dir` with the connectors wired, then an app
    /// subscription past its first frames.
    pub(crate) fn start(dir: &Dir, start: Start) -> Self {
        let fake = FakeConnector::start(FakeOptions::default()).unwrap();
        let scratch = ScratchDir::new("host-connector-state").unwrap();
        let (send, pages) = mpsc::channel();
        let events = ConnectorEvents::new();
        let mut made = None;
        let mut keys = Vec::new();
        let host = dir.start_clocked(0, |config: HostConfig, clock: &ManualClock| {
            let mut connector_config = fake.config(scratch.state_file());
            connector_config.clock = Arc::new(HostTime(clock.clone()));
            let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
            let connectors = Arc::new(Connectors::new(connector_config, store).unwrap());
            let handle = connectors.get(CASE_LAW).unwrap();
            let relays = Relays::new(RelayConfig {
                connector: handle.clone(),
                origin: fake.config(scratch.state_file()).origin,
                day_file: scratch.path().join("connectors").join("day.json"),
                clock: Arc::new(clock.clone()),
                local_day: Arc::new(FixedOffset(19_800)),
                events: Arc::<ConnectorEvents>::clone(&events),
            });
            let wiring = ConnectorsWiring {
                connectors,
                events: Arc::clone(&events),
                opener: Arc::new(Pages(Mutex::new(send))),
            };
            if start.signed_in {
                let key = fake.new_api_key();
                handle.add_api_key(&key).unwrap();
                keys.push(key);
            }
            made = Some((handle, relays.clone()));
            config.with_connectors(wiring).with_case_law_relays(relays)
        });
        let (handle, relays) = made.unwrap();
        let sub = crate::browser_install::subscribe(&host);
        let mut rig = Self {
            fake,
            handle,
            relays,
            events,
            pages,
            host,
            sub,
            seen: Vec::new(),
            keys,
            scratch_dir: scratch,
        };
        if start.signed_in {
            // Liveness, and the account (from the crate's copy the key's check made).
            rig.ok(ConnectorCommand::Check {});
            rig.state_until("signed in", |state| signed_in(state).is_some());
        }
        rig
    }

    /// Dispatches `command`, keeping the reply.
    pub(crate) fn dispatch(
        &mut self,
        command: ConnectorCommand,
    ) -> Result<CommandOutcome, HostError> {
        let reply = self.host.dispatch(HostCommand::Connector(command));
        self.keep_reply(&reply);
        reply
    }

    /// Dispatches `command` under `id`, keeping the reply.
    pub(crate) fn dispatch_as(
        &mut self,
        id: clauseo_protocol::CommandId,
        command: ConnectorCommand,
    ) -> Result<CommandOutcome, HostError> {
        let reply = self.host.dispatch_as(id, HostCommand::Connector(command));
        self.keep_reply(&reply);
        reply
    }

    fn keep_reply(&mut self, reply: &Result<CommandOutcome, HostError>) {
        let json = match reply {
            Ok(outcome) => serde_json::to_string(outcome),
            Err(error) => serde_json::to_string(error),
        };
        self.seen.push(json.unwrap());
    }

    /// Dispatches `command`, which must answer `Done`.
    pub(crate) fn ok(&mut self, command: ConnectorCommand) {
        let what = format!("{command:?}");
        let reply = self.dispatch(command);
        assert_eq!(reply, Ok(CommandOutcome::Done), "{what}");
    }

    /// The next `ConnectorsChanged` on the subscription, keeping every event read.
    pub(crate) fn next_state(&mut self, what: &str) -> ConnectorsState {
        loop {
            let item = wait(what, self.sub.next()).expect("the app subscription ended");
            self.seen.push(item.json().to_owned());
            if let HostEvent::ConnectorsChanged(state) = decode(&item) {
                return state;
            }
        }
    }

    /// The `ConnectorsChanged` states until one meets `done`, which it returns.
    pub(crate) fn state_until(
        &mut self,
        what: &str,
        done: impl Fn(&ConnectorsState) -> bool,
    ) -> ConnectorsState {
        loop {
            let state = self.next_state(what);
            if done(&state) {
                return state;
            }
        }
    }

    /// The connectors' own folder (their state file and day count).
    pub(crate) fn scratch(&self) -> &std::path::Path {
        self.scratch_dir.path()
    }

    /// The fake's account reads so far.
    pub(crate) fn account_reads(&self) -> usize {
        self.fake.requests_to("/v1/case-law/account").len()
    }

    /// A session's relay on a loopback port, and its token (as the launch prep opens one).
    pub(crate) fn relay(&self, launch: u64) -> (RelaySession, String) {
        let key = SessionKey {
            matter: MatterId::new(foreign_uuid(1)).unwrap(),
            thread: ThreadId::new(foreign_uuid(2)).unwrap(),
            launch,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (session, token) = self.relays.open(key, listener).unwrap();
        (session, token.expose().to_owned())
    }

    /// Every secret the fake issued or accepted, and the keys the test made.
    pub(crate) fn secrets(&self) -> Vec<String> {
        let mut secrets = self.fake.issued_secrets();
        secrets.extend(self.keys.iter().cloned());
        secrets
    }

    /// Stops the host, then checks that no event or reply kept holds a secret; returns them.
    pub(crate) fn finish(self) -> Vec<String> {
        self.assert_no_secret_seen();
        let Self { host, seen, .. } = self;
        host.shutdown();
        seen
    }

    /// Fails if any event or reply kept holds a secret.
    pub(crate) fn assert_no_secret_seen(&self) {
        let secrets = self.secrets();
        assert!(!secrets.is_empty(), "no secret to look for");
        for json in &self.seen {
            for secret in &secrets {
                assert!(!json.contains(secret.as_str()), "a secret in {json}");
            }
        }
    }
}

/// The case-law connector of `state`, if listed.
pub(crate) fn case_law(state: &ConnectorsState) -> Option<&ConnectorView> {
    state.connectors.iter().find(|view| view.id == CASE_LAW)
}

/// The status of the listed case-law connector.
pub(crate) fn status(state: &ConnectorsState) -> Option<&ConnectorStatus> {
    case_law(state).map(|view| &view.status)
}

/// The balance's credits when signed in with a balance.
pub(crate) fn signed_in(state: &ConnectorsState) -> Option<i32> {
    match status(state)? {
        ConnectorStatus::SignedIn {
            balance: Some(balance),
            ..
        } => Some(balance.credits),
        _ => None,
    }
}

/// A key as a window sends it.
pub(crate) fn redacted(key: &str) -> RedactedString {
    serde_json::from_value(serde_json::json!(key)).unwrap()
}

/// `AddApiKey` of `key`.
pub(crate) fn add_key(key: &str) -> ConnectorCommand {
    ConnectorCommand::AddApiKey {
        id: CASE_LAW.to_owned(),
        key: redacted(key),
    }
}

/// `SignOut`, `Remove`, `RefreshBalance`, `Add`, `AddCredits` of case law.
pub(crate) fn sign_out() -> ConnectorCommand {
    ConnectorCommand::SignOut {
        id: CASE_LAW.to_owned(),
    }
}

pub(crate) fn remove() -> ConnectorCommand {
    ConnectorCommand::Remove {
        id: CASE_LAW.to_owned(),
    }
}

pub(crate) fn refresh() -> ConnectorCommand {
    ConnectorCommand::RefreshBalance {
        id: CASE_LAW.to_owned(),
    }
}

pub(crate) fn add() -> ConnectorCommand {
    ConnectorCommand::Add {
        id: CASE_LAW.to_owned(),
    }
}

pub(crate) fn add_credits(pack: &str) -> ConnectorCommand {
    ConnectorCommand::AddCredits {
        id: CASE_LAW.to_owned(),
        pack: pack.to_owned(),
    }
}
