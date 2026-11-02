//! A client against a fresh fake: the config, the memory store, the handle, and sign-in.

use std::sync::Arc;
use std::thread;

use clauseo_connectors::fake::{FakeConnector, FakeHandle, FakeOptions, RawResponse, ScratchDir};
use clauseo_connectors::{
    Config, ConnectorHandle, Connectors, ManualClock, MemoryStore, PendingSignIn, SignInError,
};

pub(crate) struct Rig {
    pub(crate) fake: FakeHandle,
    pub(crate) store: Arc<MemoryStore>,
    pub(crate) connectors: Connectors,
    pub(crate) handle: ConnectorHandle,
    pub(crate) clock: Option<Arc<ManualClock>>,
    pub(crate) scratch: ScratchDir,
}

impl Rig {
    pub(crate) fn clock(&self) -> &ManualClock {
        self.clock.as_deref().expect("a manual-clock rig")
    }

    /// A new `Connectors` on the same fake, store and state file: an app restart.
    pub(crate) fn restart(&self) -> (Connectors, ConnectorHandle) {
        let config = self.fake.config(self.scratch.state_file());
        let connectors = Connectors::new(
            config,
            Arc::clone(&self.store) as Arc<dyn clauseo_connectors::SecretStore>,
        )
        .unwrap();
        let handle = connectors.get("case-law").unwrap();
        (connectors, handle)
    }
}

/// A rig on the system clock.
pub(crate) fn rig() -> Rig {
    rig_with(FakeOptions::default(), false, |_| {})
}

/// A rig on a manual clock shared by the fake and the client.
pub(crate) fn manual_rig() -> Rig {
    rig_with(FakeOptions::default(), true, |_| {})
}

pub(crate) fn rig_with(
    mut options: FakeOptions,
    manual: bool,
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    let clock = manual.then(|| Arc::new(ManualClock::default()));
    if let Some(clock) = &clock {
        options.clock = Arc::clone(clock) as Arc<dyn clauseo_connectors::Clock>;
    }
    let fake = FakeConnector::start(options).unwrap();
    let scratch = ScratchDir::new("client").unwrap();
    let mut config = fake.config(scratch.state_file());
    tweak(&mut config);
    let store = Arc::new(MemoryStore::new());
    let connectors = Connectors::new(
        config,
        Arc::clone(&store) as Arc<dyn clauseo_connectors::SecretStore>,
    )
    .unwrap();
    let handle = connectors.get("case-law").unwrap();
    Rig {
        fake,
        store,
        connectors,
        handle,
        clock,
        scratch,
    }
}

/// Runs `finish` on a thread and drives the browser's part at the fake: the callback's answer
/// and `finish`'s result.
pub(crate) fn complete(
    fake: &FakeHandle,
    pending: PendingSignIn,
) -> (RawResponse, Result<(), SignInError>) {
    let url = pending.authorize_url().clone();
    let finisher = thread::spawn(move || pending.finish());
    let page = fake.drive_authorize(&url).unwrap();
    (page, finisher.join().unwrap())
}

/// Signs the rig in through the whole flow.
pub(crate) fn sign_in(rig: &Rig) {
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let (page, result) = complete(&rig.fake, pending);
    assert_eq!(page.status, 200);
    result.unwrap();
}

/// The state file's text.
pub(crate) fn state_file(rig: &Rig) -> String {
    std::fs::read_to_string(rig.scratch.state_file()).unwrap_or_default()
}

/// The keychain item's JSON.
pub(crate) fn item(rig: &Rig) -> Option<serde_json::Value> {
    rig.store
        .peek("case-law")
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
}
