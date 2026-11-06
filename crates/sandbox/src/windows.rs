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
//!   on Windows, the machine's record of the pool (`sandbox_pool_sids`, `sandbox_group_sid`,
//!   `sandbox_owner_sid`, `sandbox_data_dir`) and the set-up state they give (`setup_state`).
//! - [`pool`]: the pool users' names (`ClauseoSbNN`) and the machine record's shape.
//!
//! - [`apply`](mod@apply): the plan applied through the write-ahead ledger, the walk that carries
//!   inherited entries down each tree (repairing what an agent made), the rollback, the sibling
//!   denies a new matter lifts once its first launch succeeds, `forget_matter` (which hands back
//!   what the agent made), `reconcile` and `fence_registered` (S15 PR3), with what a launch
//!   needs from around the matter as [`Surroundings`]. The writes go through [`AclWriter`]:
//!   `NtfsAcl` on Windows (`acl.rs`, after Codex's ACL code; `PROVENANCE.md`), whose
//!   descriptors and entries are edited as bytes in `acl_bytes.rs`.
//! - The set-aside (`set_aside.rs`): each protected name at the matter's root, every `.git`,
//!   `.claude` and `.mcp.json` below it, and every link in the matter tree, found by its own
//!   read-only scan and recorded by file ID at launch (`before_launch`, before `apply`), and
//!   every one new or changed set aside when the agent exits (`after_session`, after
//!   `repair_after_session` whatever it returned, or the next launch's check while any session
//!   of the matter is open), whoever owns it, into
//!   `<data_dir>\sandbox\set-aside\<matter>\<timestamp>\` at its own path below the matter,
//!   each entry in a manifest ([`SetAsideEntry`]) that `list_set_aside` shows and
//!   `restore_set_aside` undoes. A pass returns [`SetAsideChecked`]: what moved, and the
//!   folders its scan couldn't open or list.
//! - [`clean_close`]: fast later launches (S15 `PR3d`). A clean session end records the trees'
//!   file IDs and the change journal's cursor in `<data_dir>\sandbox\clean\<matter>.bin`; the
//!   next launch reads the journal from it, through `NtfsJournal` on Windows, and skips the
//!   own-tree walks and the set-aside scan when nothing in the trees changed, walks only the
//!   changed folders when a few did, and walks in full on any doubt.
//!
//! The plan, the ledger, the records, the gate's logic, the apply order and the byte edits hold
//! no Windows call, so they build and are tested on every OS; the folder check, the registry and
//! token reads, the permission writes and the walk, and the tests' DACL helper need Windows.

#[cfg(windows)]
mod acl;
#[cfg(any(windows, test))]
mod acl_bytes;
pub mod acl_plan;
pub mod admit;
pub mod apply;
pub mod clean_close;
#[cfg(windows)]
mod detect;
pub mod fence;
#[cfg(windows)]
mod folder;
pub mod ledger;
pub mod marker;
pub mod matters;
pub mod pool;
#[cfg(windows)]
mod private_dir;
#[cfg(windows)]
mod record;
mod set_aside;
pub mod shared_writes;
pub mod sids;
#[cfg(test)]
#[cfg(windows)]
mod test_acl;
#[cfg(all(windows, any(test, feature = "test-support")))]
pub mod test_dacl;
// The host's data folder fence tests make junctions too (`test-support`).
#[cfg(all(windows, any(test, feature = "test-support")))]
pub mod test_junction;
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
#[cfg(any(windows, test, feature = "test-support"))]
pub use apply::fence_registered;
pub use apply::{
    Applied, Surroundings, apply, apply_checked, forget_matter, reconcile, repair_after_session,
};
#[cfg(windows)]
pub use clean_close::NtfsJournal;
#[cfg(windows)]
pub(crate) use detect::os_facts;
pub use fence::{MatterFolders, Neighbours, RegisteredMatter, plan_fence};
#[cfg(windows)]
pub use folder::folder_support;
#[cfg(windows)]
pub use marker::setup_state;
#[cfg(windows)]
pub use record::{
    RecordError, sandbox_data_dir, sandbox_group_sid, sandbox_owner_sid, sandbox_pool_sids,
};
pub use set_aside::{
    Checked as SetAsideChecked, Entry as SetAsideEntry, Reason as SetAsideReason, SET_ASIDE_DIR,
    SetAsideFailure,
};
#[cfg(windows)]
pub use set_aside::{
    after_session, before_launch, before_launch_checked, list_set_aside, restore_set_aside,
    set_aside_path,
};
pub use writer::{AclWriter, Added, ApplyFailure, Label, Reach, Strict, Walked};

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
