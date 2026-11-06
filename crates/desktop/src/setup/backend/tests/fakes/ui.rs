//! The window-side fakes: the confirms and the opener, each recording what it was asked.

use std::path::Path;
use std::sync::Mutex;

use clauseo_protocol::HostError;

use crate::bridge::lock;
use crate::dialogs::BoxFuture;
use crate::open::Opener;
use crate::setup::confirm::{Ask, SetupConfirm};

/// The confirms, answered `answer`, each ask recorded.
pub(crate) struct FakeConfirm {
    pub(crate) answer: bool,
    pub(crate) asked: Mutex<Vec<Ask>>,
}

impl SetupConfirm for FakeConfirm {
    fn confirm(&self, ask: Ask) -> BoxFuture<'_, bool> {
        lock(&self.asked).push(ask);
        Box::pin(std::future::ready(self.answer))
    }
}

/// The opener, each URL recorded.
#[derive(Default)]
pub(crate) struct FakeOpener(pub(crate) Mutex<Vec<String>>);

impl Opener for FakeOpener {
    fn open_url(&self, url: &str) -> Result<(), HostError> {
        lock(&self.0).push(url.to_owned());
        Ok(())
    }
    fn open_path(&self, _path: &Path, _kind: &str) -> Result<(), HostError> {
        Ok(())
    }
    fn reveal(&self, _path: &Path) -> Result<(), HostError> {
        Ok(())
    }
}
