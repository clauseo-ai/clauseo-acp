//! A Claude matter stored by its network address. Claude Code 2.1.293 asks before every Read,
//! Grep and Glob of a path whose text is a UNC path ("appears to be a UNC path that could access
//! network resources"): a hard ask that auto mode, allow rules, working folders and hooks never
//! clear (its `trustedNetworkDirectories` only takes a mapped drive's UNC form). It goes by the
//! path's text, so a matter on a drive letter mapped to the share (`Z:\Matters\A`) gets no such
//! ask, and Clauseo adds nothing for it.
//!
//! For a matter stored by its address ([`by_address`]), the host answers that ask itself, once
//! and unseen, when the read stays inside the matter folder by its text ([`reads`],
//! `actor/turn/asks/share_reads.rs`) and no administrator policy tier is present
//! ([`admin_policy`]). Every change still asks.

use std::path::Path;

use clauseo_protocol::FolderSupportKind;

use super::folder_support::Answer;

/// Whether matter folder `matter` is stored by its network address, when the launch's folder
/// check `answer` found it on the network: `\\server\share\…`, `//server/share/…` or
/// `\\?\UNC\…`, but not `\\?\Z:\…` (a drive by another spelling). False when the check did not
/// find it on the network (or did not run), and off Windows. `windows`: Windows' rules.
pub(crate) fn by_address(answer: Option<Answer>, matter: &Path, windows: bool) -> bool {
    if answer != Some(Answer::Known(Some(FolderSupportKind::Network))) || !windows {
        return false;
    }
    let Some(text) = matter.to_str() else {
        return false;
    };
    let verbatim_drive = text.get(5..6) == Some(":");
    (text.starts_with(r"\\") || text.starts_with("//")) && !verbatim_drive
}

pub(crate) mod admin_policy;
pub(crate) mod reads;
#[cfg(test)]
mod tests;
