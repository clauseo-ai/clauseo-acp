//! The test build's policy file (feature `test-browser-search`, never in a release): with the
//! feature on, and only then, `CLAUSEO_TEST_POLICY_FILE` names a `policy.json` by absolute path,
//! read as the strongest policy source, ahead of the OS's. It is parsed by the same
//! [`super::file::parse`] and read through the same [`FilePolicy`] as the system file, but it has
//! no administrator-ownership check, so a real-Mac test row can run with a firm setting (say
//! `browser.allow_download = false`) on a Mac with no admin rights. A relative or empty value is
//! ignored.
//!
//! Without the feature, neither the variable's name nor any read of it is compiled in: [`source`]
//! is `None`, whatever the environment says. The release workflow refuses the feature and scans
//! the shipped binary for the variable's name.

use super::{FilePolicy, PolicySource};
use crate::files::{Files, SystemFiles};
use std::ffi::OsStr;
use std::path::PathBuf;

/// The variable the test file is named by, with the feature on.
#[cfg(feature = "test-browser-search")]
pub(super) const VAR: &str = "CLAUSEO_TEST_POLICY_FILE";

/// The test build's policy source, when the variable is an absolute path in a build with the
/// feature; `None` otherwise.
pub(super) fn source() -> Option<Box<dyn PolicySource>> {
    #[cfg(feature = "test-browser-search")]
    let value = std::env::var_os(VAR);
    #[cfg(not(feature = "test-browser-search"))]
    let value: Option<std::ffi::OsString> = None;
    source_for(value.as_deref())
}

/// The source that `value`, the variable's, names: a policy source reading that file when the
/// feature is on and `value` is an absolute path; `None` for anything else.
pub(super) fn source_for(value: Option<&OsStr>) -> Option<Box<dyn PolicySource>> {
    source_in(value, SystemFiles)
}

/// [`source_for`], reading through `files` (tests: an in-memory file system).
fn source_in<F: Files + 'static>(value: Option<&OsStr>, files: F) -> Option<Box<dyn PolicySource>> {
    let value = value.filter(|_| cfg!(feature = "test-browser-search"))?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        tracing::warn!("the test policy file is ignored: its variable is not an absolute path");
        return None;
    }
    Some(Box::new(FilePolicy::new(path, files)))
}

#[cfg(test)]
mod tests {
    use super::source_in;
    use crate::Effective;
    use crate::testing::{MemFiles, abs, locations};

    const FORBIDS: &str = r#"{"browser": {"allow_download": false}}"#;

    #[cfg(feature = "test-browser-search")]
    #[test]
    fn test_policy_file_locks_a_firm_setting_with_the_feature() {
        use crate::policy::PolicyRead;
        use std::ffi::OsStr;
        assert_eq!(super::VAR, "CLAUSEO_TEST_POLICY_FILE");
        let path = abs("/tmp/test-policy.json");
        let source = source_in(Some(path.as_os_str()), MemFiles::with(&path, FORBIDS))
            .expect("an absolute path gives a source with the feature");
        assert!(matches!(source.read(), PolicyRead::Read(_)));
        let effective = Effective::load(&locations(), &MemFiles::default(), &[&*source]);
        assert!(!effective.settings().browser.allow_download);
        assert!(effective.is_locked("browser.allow_download"));
        // A relative path, an empty value and no value give no source.
        for value in [Some(OsStr::new("policy.json")), Some(OsStr::new("")), None] {
            assert!(source_in(value, MemFiles::default()).is_none(), "{value:?}");
        }
    }

    #[cfg(not(feature = "test-browser-search"))]
    #[test]
    fn test_policy_file_is_absent_without_the_feature() {
        let path = abs("/tmp/test-policy.json");
        assert!(source_in(Some(path.as_os_str()), MemFiles::with(&path, FORBIDS)).is_none());
        assert!(super::source().is_none());
        let effective = Effective::load(&locations(), &MemFiles::default(), &[]);
        assert!(effective.settings().browser.allow_download);
        assert!(!effective.is_locked("browser.allow_download"));
    }
}
