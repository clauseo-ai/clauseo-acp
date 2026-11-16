//! Whether the keys' first load is over, for the setup commands that read or write the keys.
//!
//! The setup backend is in its cell before the keychain is read (`setup/start.rs`), so a
//! command that does not read the keys answers at once however slow the keychain is (a
//! Credential Manager read on Windows can take seconds). Only the commands [`needs_keys`]
//! names wait on [`KeysLoaded`]: what bars a sign-in (`signin.key_in_use`), whether a leave
//! leaves a usable login, and `main`'s first key all read them. That wait ends with the load,
//! which [`KEYS_LOAD_BOUND`](super::KEYS_LOAD_BOUND) bounds.

use clauseo_protocol::SetupCommand;
use futures::channel::oneshot;
use futures::future::{FutureExt as _, Shared};

/// The end of the keys' first load: every clone resolves once it is over.
#[derive(Clone)]
pub(crate) struct KeysLoaded(Shared<oneshot::Receiver<()>>);

/// Ends the first load for every [`KeysLoaded`]. Dropped unsent (the setup thread ended), the
/// waits end too, and the keys stand as far as they read.
pub(crate) struct LoadDone(oneshot::Sender<()>);

impl KeysLoaded {
    /// A load that is not over, and what ends it.
    pub(crate) fn pending() -> (Self, LoadDone) {
        let (done, wait) = oneshot::channel();
        (Self(wait.shared()), LoadDone(done))
    }

    /// A load already over: the tests' backends, whose keys loaded before them.
    #[cfg(test)]
    pub(crate) fn over() -> Self {
        let (loaded, done) = Self::pending();
        done.finish();
        loaded
    }

    /// Waits until the load is over.
    pub(crate) async fn wait(&self) {
        // Canceled (the setup thread ended) is over as well.
        let _over = self.0.clone().await;
    }
}

impl LoadDone {
    /// The load is over, and what the windows show of each key with it.
    pub(crate) fn finish(self) {
        // No receiver left only when the backend itself is gone.
        let _woken = self.0.send(());
    }
}

/// Whether `command` reads or writes the stored keys, so it waits for their first load: a
/// sign-in's start (a stored key bars it), a leave (a key is a login that remains), an endpoint
/// (it may leave a reused login) and a search for a login (its answer is composed with the key)
/// read them. A key's save or removal writes them, and waits so that the load cannot land the
/// old key after it. An install, a retry, a cancel, a pasted code, the sign-in page and a
/// sign-out neither read nor write a stored key (a sign-out ends the CLI's own login), and
/// answer at once.
pub(crate) fn needs_keys(command: &SetupCommand) -> bool {
    match command {
        SetupCommand::InstallAgent { .. }
        | SetupCommand::RetryAgentInstall { .. }
        | SetupCommand::CancelSignIn { .. }
        | SetupCommand::PasteSignInCode { .. }
        | SetupCommand::OpenSignInPage { .. }
        | SetupCommand::SignOut { .. } => false,
        SetupCommand::StartSignIn { .. }
        | SetupCommand::UseDeviceCode { .. }
        | SetupCommand::UseFoundLogin { .. }
        | SetupCommand::UseDifferentAccount { .. }
        | SetupCommand::StopUsingLogin { .. }
        | SetupCommand::FindLogin { .. }
        | SetupCommand::SetAgentKey { .. }
        | SetupCommand::ClearAgentKey { .. }
        | SetupCommand::SetFirmEndpoint { .. } => true,
    }
}
