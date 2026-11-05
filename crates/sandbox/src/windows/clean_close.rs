//! Fast later launches (S15 `PR3d`): the clean-close record and the launch check.

mod journal;
#[cfg(windows)]
mod ntfs;
// Two attributes, not `all(windows, test)`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;

pub use journal::{
    ATTRIBUTE_DIRECTORY, ATTRIBUTE_REPARSE_POINT, Change, Flow, Found, Journal, JournalError,
    JournalState, Listed, RootFacts, Usn,
};
#[cfg(windows)]
pub use ntfs::NtfsJournal;
