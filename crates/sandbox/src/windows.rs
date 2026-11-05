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
//! The plan, the ledger, the records and the gate's logic hold no Windows call, so they build and
//! are tested on every OS; the folder check, the registry and token reads, the folder's DACL and
//! the tests' DACL helper need Windows. No code here writes a permission entry yet: S15's PR3
//! applies the plan.

pub mod acl_plan;
pub mod admit;
#[cfg(windows)]
mod detect;
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
#[cfg(all(windows, any(test, feature = "test-support")))]
pub mod test_dacl;
#[cfg(test)]
mod test_world;
// Two attributes, not `all(windows, test)`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod tests;
mod write;

#[cfg(windows)]
pub use admit::admit;
#[cfg(any(test, feature = "test-support"))]
pub use admit::admit_with;
pub use admit::{Admitted, FolderList, FolderRefusal, PathKind, ReadDir};
#[cfg(windows)]
pub(crate) use detect::os_facts;
#[cfg(windows)]
pub use folder::folder_support;
#[cfg(windows)]
pub use marker::setup_state;
#[cfg(windows)]
pub use record::{RecordError, sandbox_owner_sid, sandbox_user_sid};

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
