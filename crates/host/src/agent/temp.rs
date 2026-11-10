//! Each session's own temp folder, `<data>/run/<session>/tmp` ([`temp_dir`]): made at the
//! launch and removed with the run folder, so what an agent or its tools leave in temp (a
//! compiler's module cache, an unpacked archive) goes when the session ends instead of piling up
//! in the system's temp folder. Both data-folder fences let the session write there: it is inside
//! the session's own run folder, which Codex's profile names `write` (`agent::codex_fence`) and
//! Claude's deny rules leave out (`agent::data_fence`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The session's temp folder in its run folder `run_root` (`<data>/run/<session>`).
pub(crate) fn temp_dir(run_root: &Path) -> PathBuf {
    run_root.join("tmp")
}

/// The variables that name the temp folder on macOS and Linux.
const UNIX_VARS: [&str; 1] = ["TMPDIR"];

/// The variables that name the temp folder on Windows.
const WINDOWS_VARS: [&str; 2] = ["TEMP", "TMP"];

/// What the host forces on every agent's launch: `tmp` as `TMPDIR`, or as `TEMP` and `TMP` on
/// Windows (`windows`), over any inherited, settings or hook value.
pub(crate) fn temp_env(tmp: &Path, windows: bool) -> BTreeMap<OsString, OsString> {
    let names: &[&str] = if windows { &WINDOWS_VARS } else { &UNIX_VARS };
    names
        .iter()
        .map(|name| (OsString::from(name), tmp.as_os_str().to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_temp_folder_is_the_sessions_own_on_each_os() {
        let tmp = PathBuf::from("/data/run/s1/tmp");
        let unix = temp_env(&tmp, false);
        assert_eq!(
            unix,
            BTreeMap::from([(OsString::from("TMPDIR"), tmp.clone().into_os_string())])
        );
        let windows = temp_env(&tmp, true);
        assert_eq!(windows.len(), 2);
        for name in ["TEMP", "TMP"] {
            assert_eq!(
                windows.get(&OsString::from(name)),
                Some(&tmp.clone().into_os_string())
            );
        }
    }

    #[test]
    fn two_sessions_get_two_temp_folders_in_their_run_folders() {
        let data = PathBuf::from("/data");
        let folders = |id: &str| temp_dir(&data.join("run").join(id));
        let (one, two) = (folders("s1"), folders("s2"));
        let get = |tmp: &Path| temp_env(tmp, false)[&OsString::from("TMPDIR")].clone();
        assert_ne!(get(&one), get(&two));
        assert!(Path::new(&get(&one)).starts_with(data.join("run").join("s1")));
    }
}
