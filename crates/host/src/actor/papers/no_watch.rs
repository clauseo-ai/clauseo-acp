//! Windows has no folder watch (decision record 0055): the `notify` crate is not built there, and
//! [`super::follow::opened`] starts none. This stands in for `watch.rs` so the follow code is the
//! same on every OS; a stand-in watch never reports anything.

use std::path::PathBuf;

use clauseo_protocol::MatterId;

use crate::actor::input::Input;

/// A watch that never reports.
pub(crate) struct Watch {
    pub(crate) folder: PathBuf,
}

impl Watch {
    /// No watch is started on Windows; this one holds the folder and nothing else, and, as in
    /// `watch.rs`, there is none for a folder that is not there.
    pub(crate) fn start(
        _matter: MatterId,
        folder: PathBuf,
        _post: async_channel::Sender<Input>,
    ) -> Option<Self> {
        folder.is_dir().then_some(Self { folder })
    }
}
