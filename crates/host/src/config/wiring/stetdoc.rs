//! stetdoc as the Word blackline and tracked changes run it (`HostConfig::with_stetdoc`): the
//! program, and the author the blackline's redline's changes carry. A child of `wiring.rs`,
//! which holds it, so `config.rs` stays within its size limit.
//!
//! It answers: where the host's stetdoc comes from (only from the caller: the host never looks
//! for it on `PATH`), and what a host without one does (refuses every `WordBlackline` with
//! `blackline.no_word_tool`, and answers every `ListRevisions` and `DecideRevisions` with
//! `word.stetdoc_missing`).

use std::path::PathBuf;

use crate::config::HostConfig;

/// The stetdoc the Word blackline runs (`actor/query/blackline.rs`), and tracked changes, which
/// take only its program (`actor/revisions.rs`).
#[derive(Debug, Clone)]
pub(crate) struct Stetdoc {
    /// The program, an absolute path.
    pub(crate) program: PathBuf,
    /// The author of the redline's tracked changes (`--author`), which the blackline does not
    /// show; the lawyer's name, or "Clauseo".
    pub(crate) author: String,
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "config.rs is at its 600-line limit; the Word blackline's setter lives with its type"
)]
impl HostConfig {
    /// The stetdoc the Word blackline runs (default: none, and `WordBlackline` is refused with
    /// `blackline.no_word_tool`), and the author its redline's changes carry. `program` must be
    /// absolute; the host never looks stetdoc up on `PATH`. The desktop passes the stetdoc it
    /// found for the launch profile and the profile's author. Tracked changes run the same
    /// program (without one, `word.stetdoc_missing`).
    #[must_use]
    pub fn with_stetdoc(mut self, program: PathBuf, author: String) -> Self {
        self.wiring.stetdoc = Some(Stetdoc { program, author });
        self
    }

    /// The stetdoc program and author `with_stetdoc` set, for a caller's tests.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn stetdoc(&self) -> Option<(&std::path::Path, &str)> {
        let stetdoc = self.wiring.stetdoc.as_ref()?;
        Some((stetdoc.program.as_path(), stetdoc.author.as_str()))
    }
}
