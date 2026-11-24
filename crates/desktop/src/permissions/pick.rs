//! The folder a lawyer picks, and the macOS permission its place needs. macOS guards a few places:
//! the Desktop, Downloads, the cloud folders (iCloud Drive, the File Provider ones), a removable
//! or a network volume. The first access by Clauseo, or by any process it starts, raises a dialog
//! naming Clauseo; a pick asks for the permission at once, after a reason, instead of mid-work.
//!
//! The place is classified from the path, and for a folder under `/Volumes` from the mount table
//! (`/sbin/mount`, a child process): the folder itself is never statted, opened or listed to find
//! it. Only [`ask_at_pick`] lists it, from a child process, when the lawyer presses Allow.
//! Nothing is recorded for these ids: macOS remembers its own answer. Never listed by
//! `permissions`, and never on Windows or Linux.

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, FolderRefId, HostError};

use super::is_inside;
use super::store::Answer;
use super::{FILES_SETTINGS, OsPermissionState, PermissionId, PermissionRow, Platform, Probe};

/// A mounted file system: where it is mounted, and its type's name as the kernel gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mount {
    /// Where the file system is mounted.
    pub(crate) point: PathBuf,
    /// Its type's name (`apfs`, `smbfs`, `msdos`).
    pub(crate) fs_type: String,
}

/// The file system types that live on another machine.
const NETWORK_TYPES: [&str; 5] = ["smbfs", "cifs", "afpfs", "nfs", "webdav"];
/// The mounts that hold the system and the home folders: a place on one of them is not a
/// volume's, and no permission guards it.
const SYSTEM_MOUNTS: [&str; 2] = ["/", "/System/Volumes/Data"];
/// Where macOS mounts removable and other volumes.
const VOLUMES: &str = "/Volumes";

/// The permission a folder picked at `folder` needs on this computer, with `home` the user's
/// home folder as the pick gives it; `None` when none (and always off macOS).
pub(crate) fn at_pick(
    probe: &dyn Probe,
    folder: &Path,
    home: Option<&Path>,
) -> Option<PermissionId> {
    if probe.platform() != Platform::Mac {
        return None;
    }
    classify(folder, home, || probe.mounts())
}

/// Which permission `folder` needs: from its name under `home`, else, for a folder under
/// `/Volumes`, from the mount table, which `mounts` reads only then. The paths are compared as
/// given, by component.
pub(crate) fn classify(
    folder: &Path,
    home: Option<&Path>,
    mounts: impl FnOnce() -> Vec<Mount>,
) -> Option<PermissionId> {
    home.and_then(|home| by_home(folder, home)).or_else(|| {
        is_inside(folder, Path::new(VOLUMES))
            .then(|| by_mount(folder, &mounts()))
            .flatten()
    })
}

/// The place under `home`: the Desktop, Downloads, or a cloud folder. Each is the folder itself
/// or inside it; `Desktop2` is no Desktop.
fn by_home(folder: &Path, home: &Path) -> Option<PermissionId> {
    let under = |name: &str| is_inside(folder, &home.join(name));
    if under("Desktop") {
        Some(PermissionId::FilesDesktop)
    } else if under("Downloads") {
        Some(PermissionId::FilesDownloads)
    } else if under("Library/CloudStorage") || under("Library/Mobile Documents") {
        Some(PermissionId::FilesCloud)
    } else {
        None
    }
}

/// The place on a volume: the longest mount that holds `folder`, unless it is the system's.
/// A network file system's mount is `files_network`, any other mount under `/Volumes` is
/// `files_removable`, and anything else is none.
fn by_mount(folder: &Path, mounts: &[Mount]) -> Option<PermissionId> {
    let mount = mounts
        .iter()
        .filter(|mount| is_inside(folder, &mount.point))
        .max_by_key(|mount| mount.point.components().count())?;
    if SYSTEM_MOUNTS
        .iter()
        .any(|system| mount.point == Path::new(system))
    {
        None
    } else if NETWORK_TYPES.contains(&mount.fs_type.as_str()) {
        Some(PermissionId::FilesNetwork)
    } else if is_inside(&mount.point, Path::new(VOLUMES)) {
        Some(PermissionId::FilesRemovable)
    } else {
        None
    }
}

/// The folder a pick-time ask names: `folder` is required, and its ref must be one the picker
/// issued (`lookup`), which gives the folder's path. The message names no path.
pub(crate) fn picked_folder(
    folder: Option<FolderRefId>,
    lookup: impl FnOnce(FolderRefId) -> Option<PathBuf>,
) -> Result<PathBuf, HostError> {
    let folder = folder.ok_or_else(|| {
        HostError::new(
            ErrorCode::CommandInvalid,
            "this permission needs the folder it was picked for",
        )
    })?;
    lookup(folder).ok_or_else(|| {
        HostError::new(
            ErrorCode::CommandInvalid,
            format!("folder ref {folder} was not picked in this run"),
        )
    })
}

/// Asks for the pick-time `id` for `folder`: lists the folder from a child process, as long as
/// the lawyer takes with macOS's dialog, and answers its row. Granted when the listing works;
/// otherwise denied, with the settings' place. `command.invalid` off macOS and for an id that is
/// not a pick-time one.
pub(crate) fn ask_at_pick(
    probe: &dyn Probe,
    id: PermissionId,
    folder: &Path,
) -> Result<PermissionRow, HostError> {
    if !id.is_pick_time() || probe.platform() != Platform::Mac {
        return Err(HostError::new(
            ErrorCode::CommandInvalid,
            format!("{id:?} is not needed on this computer"),
        ));
    }
    let state = Answer::from_granted(probe.list_folder(folder)?).state();
    tracing::info!(?id, ?state, "permissions: asked for a picked folder");
    Ok(PermissionRow {
        id,
        state,
        folder: Some(folder.display().to_string()),
        settings_path: (state == OsPermissionState::Denied).then(|| FILES_SETTINGS.to_owned()),
        note: None,
        asked: true,
    })
}

/// The mounts `/sbin/mount` lists, one `<device> on <point> (<fstype>, <flags>)` a line. The point
/// is what lies between the first ` on ` and the last ` (`, so a point with spaces stays whole. A
/// line that does not parse is skipped.
pub(crate) fn parse_mounts(listing: &str) -> Vec<Mount> {
    listing.lines().filter_map(parse_mount).collect()
}

/// One line of [`parse_mounts`]; `None` when it has no point or no type.
fn parse_mount(line: &str) -> Option<Mount> {
    let (_device, rest) = line.trim_end().split_once(" on ")?;
    let (point, options) = rest.rsplit_once(" (")?;
    let fs_type = options.strip_suffix(')')?.split(',').next()?.trim();
    if point.is_empty() || fs_type.is_empty() {
        return None;
    }
    Some(Mount {
        point: PathBuf::from(point),
        fs_type: fs_type.to_owned(),
    })
}
