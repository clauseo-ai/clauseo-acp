//! The real-Mac test row's switch (feature `test-browser-search`, never in a release): with the
//! feature on, and only then, `CLAUSEO_TEST_IGNORE_INSTALLED_BROWSERS=1` makes the macOS and
//! Linux search pass over the browsers installed on the machine, so a Mac that has Chrome behaves
//! as one with none and the download's offer shows, while the copy Clauseo downloaded stays a
//! candidate (so a second start reuses it). Nothing on the machine is touched.
//!
//! Without the feature, neither the variable's name nor any read of it is compiled in:
//! [`ignore_installed`] is `false`, whatever the environment says. The release workflow refuses
//! the feature and scans the shipped binary for the variable's name.

use std::ffi::OsStr;

/// The variable the switch reads, with the feature on.
#[cfg(feature = "test-browser-search")]
pub(crate) const VAR: &str = "CLAUSEO_TEST_IGNORE_INSTALLED_BROWSERS";

/// Whether this launch's search passes over the installed browsers: the variable is `1`, in a
/// build with the feature. Read at each search, off the actor.
pub(crate) fn ignore_installed() -> bool {
    #[cfg(feature = "test-browser-search")]
    let value = std::env::var_os(VAR);
    #[cfg(not(feature = "test-browser-search"))]
    let value: Option<std::ffi::OsString> = None;
    says(value.as_deref())
}

/// Whether `value`, the variable's, turns the switch on: exactly `1` with the feature, never
/// without it.
pub(crate) fn says(value: Option<&OsStr>) -> bool {
    cfg!(feature = "test-browser-search") && value == Some(OsStr::new("1"))
}
