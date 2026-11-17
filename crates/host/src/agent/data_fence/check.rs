//! The matter thread's check of a permission request's paths ([`check_paths`]): the data
//! folder, the other matters' folders (Codex only), then whether a program holds one open.

use std::path::{Path, PathBuf};

use clauseo_matters::{HeldOpen, held_open_resolved};
use clauseo_protocol::MatterPath;

use super::FenceCheck;
use super::resolve::Disk;
use crate::network_paths::{FOLD_CASE, NetEnv, touchable};

/// What the matter thread found for a permission request's paths.
#[derive(Debug)]
pub(crate) enum PathsChecked {
    /// One resolves into Clauseo's data folder, outside the session's own folders.
    DataFolder,
    /// One resolves into another matter's folder ([`super::other_matters`]).
    OtherMatter,
    /// The first that a program holds open, if any (`clauseo_matters::held_open_at`).
    Held(Option<(MatterPath, HeldOpen)>),
}

/// The data-folder check, then the held-open check of `paths` in the matter `folder`. A path
/// that is or leads to a network location, or lies by its text in a matter folder on a share,
/// is never looked at for being held; one that may be is looked at by the path the network-safe
/// walk resolved, never by its text again ([`touchable`]).
pub(crate) fn check_paths(
    folder: &Path,
    paths: Vec<PathBuf>,
    fence: Option<&FenceCheck>,
) -> PathsChecked {
    let net = NetEnv::current();
    let disk = Disk::real(&net);
    if fence.is_some_and(|fence| fence.touches_data_on(FOLD_CASE, &disk)) {
        return PathsChecked::DataFolder;
    }
    let others = fence.and_then(|fence| Some((fence, fence.other_matters.as_ref()?)));
    if others.is_some_and(|(fence, others)| others.reached(fence, FOLD_CASE, &disk)) {
        return PathsChecked::OtherMatter;
    }
    let walk = Disk::resolved(&net);
    PathsChecked::Held(
        paths
            .into_iter()
            .filter_map(|path| touchable(&path, folder, &walk))
            .find_map(|walked| held_open_resolved(folder, &walked)),
    )
}
