//! What turning a pack on and listing packs need beyond [`super::inspect`]: a read that tells a
//! pack that changed while it was read ([`inspect_twice`]), the wire's mirror of [`Refusal`],
//! and the filter a manifest's display text passes before it reaches a window ([`listed`]).

use std::path::Path;

use clauseo_protocol::PackRefusal;

use super::{Pack, PackFolders, Refusal, inspect, inspect_in};
use crate::skills::builtin::is_listable;

/// The folder under the data folder holding the packs: `<data>/skill-packs/`.
pub const PACKS_DIR: &str = "skill-packs";

/// Why [`inspect_twice`] gave no pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    /// The first read refused it.
    Refused(Refusal),
    /// Its files changed while they were read: the two reads hashed differently, or the second
    /// was refused after the first passed.
    Changed,
}

impl ReadError {
    /// The reason as a short code, for a log line (no path, no pack text).
    pub fn code(self) -> &'static str {
        match self {
            Self::Refused(refusal) => refusal.code(),
            Self::Changed => "changed",
        }
    }
}

/// [`inspect`] twice, the pack kept only when both reads give the same hash. One read takes
/// each file in turn, so a pack edited mid-read can hash to files that never stood together; a
/// second read that agrees shows the folder held still across the first. It costs a second
/// read, at most [`super::PACK_MAX`] bytes. Off the host thread.
pub fn inspect_twice(packs_root: &Path, id: &str, allow_scripts: bool) -> Result<Pack, ReadError> {
    inspect_twice_with(packs_root, id, allow_scripts, &|| {})
}

/// [`inspect_twice`] with `between` run after the first read and before the second: the tests'
/// way to change a pack mid-read, or to see which thread reads.
#[doc(hidden)]
pub fn inspect_twice_with(
    packs_root: &Path,
    id: &str,
    allow_scripts: bool,
    between: &dyn Fn(),
) -> Result<Pack, ReadError> {
    twice(&|| inspect(packs_root, id, allow_scripts), between)
}

/// [`inspect_twice_with`], each read finding the pack's folder in `folders`, a listing taken
/// once for many packs (the list of packs), so reading every pack stays linear.
pub fn inspect_twice_in(
    folders: &PackFolders,
    id: &str,
    allow_scripts: bool,
    between: &dyn Fn(),
) -> Result<Pack, ReadError> {
    twice(&|| inspect_in(folders, id, allow_scripts), between)
}

/// `read` twice, `between` between, the pack kept when both reads hash the same.
fn twice(read: &dyn Fn() -> Result<Pack, Refusal>, between: &dyn Fn()) -> Result<Pack, ReadError> {
    let first = read().map_err(ReadError::Refused)?;
    between();
    match read() {
        Ok(second) if second.hash() == first.hash() => Ok(second),
        Ok(_) | Err(_) => Err(ReadError::Changed),
    }
}

/// `text` as a window may show it: none when it holds a control, format or other unlisted
/// character, or text kept out of instructions (0031's index filter), else its first
/// `max_chars` characters.
pub fn listed(text: &str, max_chars: usize) -> Option<String> {
    is_listable(text).then(|| text.chars().take(max_chars).collect())
}

impl From<Refusal> for PackRefusal {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Missing => Self::Missing,
            Refusal::Link => Self::Link,
            Refusal::HardLink => Self::HardLink,
            Refusal::NotAFile => Self::NotAFile,
            Refusal::BadPath => Self::BadPath,
            Refusal::Unlisted => Self::Unlisted,
            Refusal::TooBig => Self::TooBig,
            Refusal::Manifest => Self::Manifest,
            Refusal::BadId => Self::BadId,
            Refusal::IdMismatch => Self::IdMismatch,
            Refusal::Script => Self::Script,
            Refusal::ScriptInInstructionOnly => Self::ScriptInInstructionOnly,
            Refusal::Skill => Self::Skill,
            Refusal::Io => Self::Io,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_drops_control_and_format_characters_and_caps() {
        assert_eq!(listed("NDA kit", 200).as_deref(), Some("NDA kit"));
        assert_eq!(listed("abcdef", 3).as_deref(), Some("abc"));
        for bad in ["a\u{202E}b", "a\u{200B}b", "a\nb", "a\u{FE0F}"] {
            assert_eq!(listed(bad, 200), None, "{bad:?}");
        }
    }
}
