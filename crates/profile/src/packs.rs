//! Skill packs (decision record 0040): a folder holding `clauseo-pack.json` and
//! `skills/<skill>/SKILL.md`, turned on by the lawyer with its content hash, read and checked
//! before every launch, and written into each launch's own plugin from the bytes just read.
//!
//! - [`inspect`] reads one pack into memory once ([`read`]), parses its manifest
//!   ([`manifest`]), hashes the bytes read ([`hash`]), checks the layout and the scripts
//!   ([`scripts`]) and renders each skill as `<pack>--<skill>` ([`render`]). What is hashed is
//!   what is checked and what is rendered: there is one read.
//! - [`load`] does it for a launch's packs, in parallel, against the hashes recorded when each
//!   was turned on, and returns what loaded and what was skipped and why.
//! - [`write_packs`] writes the loaded packs into a launch's own Claude plugin, a fresh copy
//!   from memory each launch, and returns the [`PacksWritten`] token the Codex index and Claude's
//!   `_meta` need ([`plugin`], [`index`]).
//!
//! Nothing here starts a process (tested). A pack's files are copied, never run, and never get
//! the exec bit.

pub mod hash;
pub mod index;
pub mod manifest;
pub mod plugin;
pub mod read;
pub mod render;
pub mod scripts;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::path::{Path, PathBuf};

use crate::skills::builtin::BUILTIN;
use crate::skills::stetdoc::STETDOC;

pub use hash::PackHash;
pub use manifest::PackManifest;
pub use plugin::{PacksWritten, write_packs};
pub use render::{Pack, PackSkill};

/// The manifest's file name, at the top of a pack.
pub const PACK_MANIFEST: &str = "clauseo-pack.json";

/// The folder under a pack holding its skills.
pub const SKILLS_DIR: &str = "skills";

/// What joins a pack's id and a skill's name in the rendered name: `<pack>--<skill>`.
pub const SEPARATOR: &str = "--";

/// The most bytes one file of a pack has.
pub const FILE_MAX: u64 = 1024 * 1024;

/// The most bytes `clauseo-pack.json` has.
pub const MANIFEST_MAX: u64 = 64 * 1024;

/// The most bytes all of a pack's files have together.
pub const PACK_MAX: u64 = 8 * 1024 * 1024;

/// The most files a pack has.
pub const FILES_MAX: usize = 256;

/// The most folders a pack holds, its own not counted (on every OS).
pub const FOLDERS_MAX: usize = 64;

/// The most folders a file's path inside a pack crosses (`skills/a/b/x.md` crosses 3).
pub const DEPTH_MAX: usize = 8;

/// The most bytes a file's path inside a pack has, so a rendered path stays short on Windows.
pub const PATH_MAX: usize = 160;

/// Reserved pack ids that are neither built-in skill names nor `clauseo-*`: the built-in pack's
/// id and the bare `builtin`.
const RESERVED: [&str; 2] = ["clauseo", "builtin"];

/// The most skills a pack lists.
pub const SKILLS_MAX: usize = 32;

/// The most extra packs one launch loads; the rest are skipped as [`Skip::TooMany`].
pub const PACKS_PER_LAUNCH: usize = 16;

/// Whether `id` may name a pack: the shape is [`PackId`](clauseo_protocol::PackId)'s
/// (`^[a-z0-9]+(-[a-z0-9]+)*$`, 2 to 30 bytes, no Windows device name), and it is not reserved:
/// not `clauseo`, not `clauseo-*`, not `builtin`, not `stetdoc`, not a built-in skill's name.
/// Since no id holds `--`, the rendered `<pack>--<skill>` splits one way only and never equals
/// a built-in name. The host refuses a reserved id in a matter's list with this same check.
pub fn is_pack_id(id: &str) -> bool {
    clauseo_protocol::PackId::new(id).is_ok() && !is_reserved(id)
}

/// Whether `id` is reserved for Clauseo's own packs and skills.
fn is_reserved(id: &str) -> bool {
    RESERVED.contains(&id) || id.starts_with("clauseo-") || id == STETDOC || BUILTIN.contains(&id)
}

/// Whether `name` may name a skill inside a pack: [`is_pack_id`]'s charset and no `--`, 1 to
/// 64 bytes before the pack's id is added; [`render`] checks the rendered name's length.
pub fn is_pack_skill_name(name: &str) -> bool {
    !name.is_empty() && is_part(name) && crate::skills::is_skill_name(name)
}

/// `^[a-z0-9]+(-[a-z0-9]+)*$`, not a Windows device name.
fn is_part(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('-')
        && !text.ends_with('-')
        && !text.contains(SEPARATOR)
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && crate::skills::is_skill_name(text)
}

/// Why a pack is refused. Each has a [`Self::code`] for log lines and notices, which never
/// hold a path or the pack's own text (decision record 0047).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// No folder named exactly the id in the packs folder, or it is not a folder.
    Missing,
    /// A symlink, a junction or any other reparse point inside the pack, or at it.
    Link,
    /// A file with more than one hard link.
    HardLink,
    /// Something that is neither a file nor a folder (a pipe, a socket, a device).
    NotAFile,
    /// A name that is not UTF-8, or a path that is not plain, too long, too deep, or given
    /// twice in different cases.
    BadPath,
    /// A file or folder the layout does not allow, or a listed skill with no `SKILL.md`.
    Unlisted,
    /// Past a size or count cap.
    TooBig,
    /// `clauseo-pack.json` does not parse, has an unknown key or a bad field.
    Manifest,
    /// The manifest's id is not a pack id, or is reserved.
    BadId,
    /// The folder's name is not the manifest's id, byte for byte.
    IdMismatch,
    /// A script or executable, and `skills.allow_scripts` is off.
    Script,
    /// A script or executable in a pack whose manifest says `instruction_only`.
    ScriptInInstructionOnly,
    /// A skill's `SKILL.md` breaks a rule, or a file holds a shell injection.
    Skill,
    /// A read failed.
    Io,
}

impl Refusal {
    /// The reason as a short code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Link => "link",
            Self::HardLink => "hard_link",
            Self::NotAFile => "not_a_file",
            Self::BadPath => "bad_path",
            Self::Unlisted => "unlisted",
            Self::TooBig => "too_big",
            Self::Manifest => "manifest",
            Self::BadId => "bad_id",
            Self::IdMismatch => "id_mismatch",
            Self::Script => "script",
            Self::ScriptInInstructionOnly => "script_in_instruction_only",
            Self::Skill => "skill",
            Self::Io => "io",
        }
    }
}

/// A `map_err` closure that turns any error into `refusal`. A refusal carries its code alone,
/// never the error's text, which may quote the pack or name a path (decision record 0047).
pub(crate) fn refuse<E>(refusal: Refusal) -> impl FnOnce(E) -> Refusal {
    move |_| refusal
}

/// Why [`load`] left a pack out of a launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Skip {
    /// The pack is refused ([`inspect`]).
    Refused(Refusal),
    /// Its hash is not the one recorded when it was turned on.
    Changed,
    /// Past [`PACKS_PER_LAUNCH`].
    TooMany,
}

impl Skip {
    /// The reason as a short code, for a log line (no path, no pack text).
    pub fn code(self) -> &'static str {
        match self {
            Self::Refused(refusal) => refusal.code(),
            Self::Changed => "changed",
            Self::TooMany => "too_many",
        }
    }
}

/// Reads, checks, hashes and renders the pack `id` in `packs_root`, all from one read into
/// memory. The folder must be listed by `read_dir` under exactly the name `id`, so a disk that
/// ignores case never matches `NDA-Kit` to `nda-kit`. Pure apart from the reads: no write, no
/// process. Off the host thread.
pub fn inspect(packs_root: &Path, id: &str, allow_scripts: bool) -> Result<Pack, Refusal> {
    if !is_pack_id(id) {
        return Err(Refusal::BadId);
    }
    let folder = pack_folder(packs_root, id)?;
    let files = read::read_pack(&folder)?;
    render::check_and_render(id, files, allow_scripts)
}

/// `packs_root/<id>`, when `read_dir` lists exactly that name.
fn pack_folder(packs_root: &Path, id: &str) -> Result<PathBuf, Refusal> {
    let entries = std::fs::read_dir(packs_root).map_err(refuse(Refusal::Missing))?;
    for entry in entries {
        let entry = entry.map_err(refuse(Refusal::Io))?;
        if entry.file_name().as_encoded_bytes() == id.as_bytes() {
            return Ok(entry.path());
        }
    }
    Err(Refusal::Missing)
}

/// One pack a launch asks for: its id and the hash recorded when it was turned on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackEntry {
    /// The pack's id.
    pub id: String,
    /// The hash `skills.enabled` holds for it.
    pub hash: PackHash,
}

/// What a launch's packs came to: those loaded, in the order asked, and those skipped.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Loaded {
    /// Packs read, checked, hashed and rendered, in memory.
    pub packs: Vec<Pack>,
    /// Packs left out, by id, with the reason.
    pub skipped: Vec<(String, Skip)>,
}

/// Loads `entries` (the launch's packs, in `skills.enabled` order) from `packs_root`: each is
/// [`inspect`]ed and its hash compared with the one recorded. Packs past [`PACKS_PER_LAUNCH`]
/// are skipped as [`Skip::TooMany`]. The packs are read in parallel, one thread each, so a
/// launch waits for the largest, not the sum. Each skip is logged with the id and its code
/// alone. Off the host thread.
pub fn load(packs_root: &Path, entries: &[PackEntry], allow_scripts: bool) -> Loaded {
    let (now, later) = entries.split_at(entries.len().min(PACKS_PER_LAUNCH));
    let results: Vec<Result<Pack, Skip>> = std::thread::scope(|scope| {
        // Every thread starts before the first is joined.
        let mut handles = Vec::with_capacity(now.len());
        for entry in now {
            handles.push(scope.spawn(move || load_one(packs_root, entry, allow_scripts)));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or(Err(Skip::Refused(Refusal::Io))))
            .collect()
    });
    let mut loaded = Loaded::default();
    for (entry, result) in now.iter().zip(results) {
        match result {
            Ok(pack) => loaded.packs.push(pack),
            Err(skip) => loaded.skipped.push((entry.id.clone(), skip)),
        }
    }
    loaded
        .skipped
        .extend(later.iter().map(|entry| (entry.id.clone(), Skip::TooMany)));
    for (id, skip) in &loaded.skipped {
        if is_pack_id(id) {
            tracing::warn!(
                pack = id.as_str(),
                reason = skip.code(),
                "a skill pack is not loaded"
            );
        } else {
            tracing::warn!(
                reason = skip.code(),
                "a skill pack with a bad id is not loaded"
            );
        }
    }
    loaded
}

/// [`load`] for one pack.
fn load_one(packs_root: &Path, entry: &PackEntry, allow_scripts: bool) -> Result<Pack, Skip> {
    let pack = inspect(packs_root, &entry.id, allow_scripts).map_err(Skip::Refused)?;
    if *pack.hash() != entry.hash {
        return Err(Skip::Changed);
    }
    Ok(pack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_rule() {
        for good in ["nda-kit", "ab", "a1-b2-c3", "x0"] {
            assert!(is_pack_id(good), "{good}");
        }
        let long = "a".repeat(31);
        for bad in [
            "a",
            "-ab",
            "ab-",
            "a--b",
            "Ab",
            "a_b",
            "a.b",
            "nul",
            "com1",
            "clauseo",
            "clauseo-x",
            "builtin",
            "stetdoc",
            "legal-drafting",
            &long,
            "",
        ] {
            assert!(!is_pack_id(bad), "{bad}");
        }
        assert!(is_pack_skill_name("nda-review"));
        assert!(!is_pack_skill_name("nda--review"));
    }
}
