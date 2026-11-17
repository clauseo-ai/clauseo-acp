//! [`Matter::restore`], apart from `matter.rs` to keep that file under the size limit.

use super::{Matter, json};
use crate::types::{Restore, Restored};
use crate::{Error, restore};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "matter.rs is at its 600-line limit; the restore lives in its own file"
)]
impl Matter {
    /// Restores version `request.from`: the whole matter or chosen files. Checks the request,
    /// plans against the folder and checks every target and every lock before committing or
    /// writing anything (`matter.file_locked` names the locked files); takes an `outside-edits`
    /// checkpoint if the folder changed since the last checkpoint; writes each file through a
    /// synced temp file, checks that no target changed since that checkpoint, renames them into
    /// place, and takes a `restore` checkpoint (no thread, no turn). Files added since stay, and
    /// nothing is deleted, but the files a restore it undoes added ([`Restore::undoes`]).
    /// Cloud-only, over-cap and excluded files are never read or written. See [`Restore`] for
    /// the ids and retrying, [`RestorePlan`](crate::types::RestorePlan) for what is kept.
    pub fn restore(&mut self, request: &Restore<'_>) -> Result<Restored, Error> {
        json::check(&self.folder, self.id)?;
        restore::restore(&mut self.core, &self.folder, &self.settings, request)
    }
}
