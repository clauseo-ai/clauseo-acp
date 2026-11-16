//! The folders a new matter's folder must not be, hold or sit inside, and the check an import
//! runs on its worker after the place check (`MatterJob::Import`).

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use clauseo_protocol::{CommandId, ErrorCode, HostError, MatterId};

use crate::error::Error;

/// How a folder of the registry is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Claim {
    /// A matter on Clauseo's list.
    Listed,
    /// A matter being made by a create or import in flight, or a relocate's target.
    Making,
    /// A forgotten matter: its own folder brings it back on import.
    Forgotten,
}

/// One folder a new matter's folder must not be, hold or sit inside.
#[derive(Debug, Clone)]
pub(crate) struct Claimed {
    pub(crate) matter: MatterId,
    pub(crate) folder: PathBuf,
    pub(crate) claim: Claim,
    /// For [`Claim::Making`] by a create or import: its command, which a second create or
    /// import of the same folder joins. `None` for a relocate's target and the other claims.
    pub(crate) command: Option<CommandId>,
}

/// What [`check_registry`] found when it does not refuse the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Held {
    /// No folder of the registry is, holds or sits inside it (or, for an import, it is a
    /// forgotten matter's own folder, which the import brings back).
    Free,
    /// It is this listed matter's own folder, by any spelling: an import answers with that
    /// matter and makes nothing.
    Listed(MatterId),
    /// It is the folder this create or import in flight is making: the new command joins it.
    Making(CommandId),
}

/// A refusal when `folder` is, holds or sits inside a folder of `registry`, compared as the
/// refused-folder check compares (`same_or_nested`): `matter.folder_inside_matter` inside it,
/// `matter.folder_holds_matter` holding it, `matter.folder_is_matter` the same folder. It
/// names the matter (`HostError::matter`) only when it is on the list: there is nothing to open
/// otherwise. The
/// same folder is not refused when something else answers for it ([`Held`]): one a create or
/// import in flight is making, and for an import (`import`) a listed matter's own folder (the
/// import answers with that matter) or a forgotten matter's (the import finds its marker and
/// brings that matter back).
pub(crate) fn check_registry(
    folder: &Path,
    registry: &[Claimed],
    import: bool,
) -> Result<Held, Error> {
    for claimed in registry {
        if !clauseo_matters::same_or_nested(folder, &claimed.folder) {
            continue;
        }
        let same = || same_folder(folder, &claimed.folder);
        match (claimed.claim, claimed.command) {
            (Claim::Making, Some(command)) if same() => return Ok(Held::Making(command)),
            (Claim::Listed, _) if import && same() => return Ok(Held::Listed(claimed.matter)),
            (Claim::Forgotten, _) if import && same() => continue,
            _ => {}
        }
        let (code, relation) = match nesting(folder, &claimed.folder) {
            Nesting::Same => (ErrorCode::MatterFolderIsMatter, "is"),
            Nesting::Inside => (ErrorCode::MatterFolderInsideMatter, "sits inside"),
            Nesting::Holds => (ErrorCode::MatterFolderHoldsMatter, "holds"),
        };
        let refusal = HostError::new(
            code,
            format!(
                "the folder {relation} the folder of matter {} ({:?})",
                claimed.matter, claimed.claim
            ),
        );
        let refusal = match claimed.claim {
            Claim::Listed => refusal.with_matter(claimed.matter),
            Claim::Making | Claim::Forgotten => refusal,
        };
        return Err(Error::Refused(refusal));
    }
    Ok(Held::Free)
}

/// Whether `a` and `b` are the same folder by any spelling: `same_or_nested` relates them and
/// neither holds the other ([`same_folder`]). For an import, which answers a listed matter's
/// own folder with that matter.
pub(crate) fn is_same_folder(a: &Path, b: &Path) -> bool {
    clauseo_matters::same_or_nested(a, b) && same_folder(a, b)
}

/// How a folder relates to a folder of the registry that `same_or_nested` relates it to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nesting {
    /// The same folder.
    Same,
    /// The folder sits inside the registry's.
    Inside,
    /// The folder holds the registry's.
    Holds,
}

/// How `folder` relates to `claimed`, two folders `same_or_nested` relates: by the number of
/// components of their paths, canonicalised (as written when that fails).
fn nesting(folder: &Path, claimed: &Path) -> Nesting {
    match depth(folder).cmp(&depth(claimed)) {
        Ordering::Equal => Nesting::Same,
        Ordering::Greater => Nesting::Inside,
        Ordering::Less => Nesting::Holds,
    }
}

/// Whether two folders that `same_or_nested` relates are the same folder, not one inside the
/// other: their paths, canonicalised (as written when that fails), have as many components.
fn same_folder(a: &Path, b: &Path) -> bool {
    depth(a) == depth(b)
}

/// The number of components of `path`, canonicalised (as written when that fails).
fn depth(path: &Path) -> usize {
    dunce::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .components()
        .count()
}
