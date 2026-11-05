//! Windows: the agent sandbox that grants by ACL (tier B, S15).
//!
//! - [`folder_support`] (Windows only, `folder.rs`): whether a folder can take the agent's
//!   permission entries: network, no permissions, or no `WRITE_DAC`.
//! - [`admit`] (`admit.rs`): the folder gate. The matter must not be, hold or sit inside
//!   Clauseo's data folder, and every folder tier B would grant on must answer
//!   [`FolderSupport::Ok`], or the launch is refused with
//!   [`Error::FolderUnsupported`](crate::Error::FolderUnsupported) and nothing is planned or
//!   written. The sibling folders the pool users are denied are listed here too.
//! - [`acl_plan`]: the permission entries a launch needs, planned from what the gate admitted,
//!   with no IO.
//! - [`ledger`]: `<data_dir>\sandbox\acl-ledger.json`, the write-ahead list of every entry
//!   written, so set-up can remove them (D4).
//! - Tier B's records, in `<data_dir>\sandbox\` under a protected DACL: each matter's SID in
//!   `matters.json` ([`matters`], [`sids`]) and the set-up marker `setup.json` ([`marker`]);
//!   on Windows, the machine's sandbox user and owner records (`sandbox_user_sid`,
//!   `sandbox_owner_sid`) and the set-up state they give (`setup_state`).
//!
//! - [`writer`]: the permission writes as a seam, [`AclWriter`]. `NtfsAcl` (Windows, `acl.rs`,
//!   after Codex's ACL code; `PROVENANCE.md`) writes entries and labels without following a
//!   link and walks each tree to carry inherited entries down; `acl_bytes.rs` edits the
//!   descriptors and entries as bytes. S15's `PR3b` applies the plan through it.
//!
//! The plan, the ledger, the records, the gate's logic and the byte edits hold no Windows call,
//! so they build and are tested on every OS; the folder check, the registry and token reads, the
//! permission writes and the walk, and the tests' DACL helper need Windows.

#[cfg(windows)]
mod acl;
#[cfg(any(windows, test))]
mod acl_bytes;
pub mod acl_plan;
pub mod admit;
#[cfg(windows)]
mod detect;
pub mod fence;
#[cfg(windows)]
mod folder;
pub mod ledger;
pub mod marker;
pub mod matters;
#[cfg(windows)]
mod private_dir;
#[cfg(windows)]
mod record;
pub mod sids;
#[cfg(test)]
#[cfg(windows)]
mod test_acl;
#[cfg(all(windows, any(test, feature = "test-support")))]
pub mod test_dacl;
// Two attributes, as for `tests` below.
#[cfg(test)]
#[cfg(windows)]
mod test_junction;
#[cfg(test)]
mod test_world;
// Two attributes, not `all(windows, test)`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod tests;
mod write;
pub mod writer;

#[cfg(windows)]
pub use acl::NtfsAcl;
#[cfg(windows)]
pub use admit::admit;
#[cfg(any(test, feature = "test-support"))]
pub use admit::admit_with;
pub use admit::{Admitted, FolderList, FolderRefusal, PathKind, ReadDir};
#[cfg(windows)]
pub(crate) use detect::os_facts;
pub use fence::{MatterFolders, Neighbours, RegisteredMatter, plan_fence};
#[cfg(windows)]
pub use folder::folder_support;
#[cfg(windows)]
pub use marker::setup_state;
#[cfg(windows)]
pub use record::{RecordError, sandbox_owner_sid, sandbox_user_sid};
pub use writer::{AclWriter, Added, ApplyFailure, Reach, Walked};

/// Whether an agent sandbox that grants by ACL can prepare a folder on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderSupport {
    /// A local volume with permissions, and the caller can change the folder's permissions.
    Ok,
    /// The caller can't change the folder's permissions: the folder's DACL doesn't grant the
    /// caller `WRITE_DAC`, or the permissions can't be read.
    CantChangePermissions,
    /// The folder is on a network drive: a UNC path, a mapped drive, or a link that resolves to
    /// one.
    Network,
    /// The folder's volume keeps no permissions (FAT, exFAT).
    NoPermissions,
}
