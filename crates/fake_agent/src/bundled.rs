//! The compiled-in scenarios, and the paths other crates' tests use.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::Error;

/// Lists every bundled scenario and common include once. Expands to [`SCENARIOS`], the texts,
/// and [`COMMON`]. Keep each list sorted; `bundled_names_match_scenario_files` checks both
/// against the folders.
macro_rules! bundled {
    (scenarios: [$($name:literal),* $(,)?], common: [$($common:literal),* $(,)?] $(,)?) => {
        /// The names of the bundled scenarios, sorted. Each is a file stem in `scenarios/`.
        pub const SCENARIOS: &[&str] = &[$($name),*];

        const TEXTS: &[&str] =
            &[$(include_str!(concat!("../scenarios/", $name, ".jsonl"))),*];

        /// The files in `scenarios/common/`, by file name, sorted.
        pub(crate) const COMMON: &[&str] = &[$(concat!($common, ".jsonl")),*];

        const COMMON_TEXTS: &[&str] =
            &[$(include_str!(concat!("../scenarios/common/", $common, ".jsonl"))),*];
    };
}

bundled! {
    scenarios: [
        "exit-mid-turn",
        "files-multi-turn",
        "grandchild",
        "idle",
        "ignore-stdin-close",
        "initialize-caps",
        "malformed-line",
        "permission-cancelled",
        "prompt-stream",
    ],
    common: ["handshake"],
}

/// The text of bundled scenario `name`.
pub(crate) fn text(name: &str) -> Option<&'static str> {
    SCENARIOS
        .iter()
        .position(|n| *n == name)
        .and_then(|i| TEXTS.get(i).copied())
}

/// The text of common include `file` (a file name such as `handshake.jsonl`).
pub(crate) fn common(file: &str) -> Option<&'static str> {
    COMMON
        .iter()
        .position(|n| *n == file)
        .and_then(|i| COMMON_TEXTS.get(i).copied())
}

/// The absolute path of bundled scenario `name`'s source file, for tests that read it.
pub fn scenario_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scenarios")
        .join(format!("{name}.jsonl"))
}

/// The `fake_agent` binary for another crate's tests: `CLAUSEO_FAKE_AGENT_BIN` if set, else
/// `fake_agent` in the target profile folder of the running test binary.
///
/// Cargo does not build a dependency's binaries for `cargo nextest run -p <other>`; the error
/// says to run `cargo build -p clauseo-fake_agent`.
pub fn bin_path() -> Result<PathBuf, Error> {
    let exe = std::env::current_exe().map_err(|source| Error::Io {
        path: PathBuf::from("current_exe"),
        source,
    })?;
    let path = bin_path_from(
        std::env::var_os("CLAUSEO_FAKE_AGENT_BIN"),
        &exe,
        std::env::consts::EXE_SUFFIX,
    );
    if path.is_file() {
        Ok(path)
    } else {
        Err(Error::BinaryNotFound { path })
    }
}

/// Where [`bin_path`] looks, given the override, the running executable and the platform's
/// executable suffix. A test binary lives in `target/<profile>/deps/`.
fn bin_path_from(var: Option<OsString>, exe: &Path, suffix: &str) -> PathBuf {
    if let Some(var) = var.filter(|v| !v.is_empty()) {
        return PathBuf::from(var);
    }
    let mut folder = exe.parent().unwrap_or_else(|| Path::new("")).to_owned();
    if folder.file_name().is_some_and(|name| name == "deps") {
        folder.pop();
    }
    folder.join(format!("fake_agent{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stems(folder: &Path) -> Vec<String> {
        let mut stems: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        stems.sort();
        stems
    }

    #[test]
    fn bundled_names_match_scenario_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");
        assert!(
            SCENARIOS.is_sorted(),
            "SCENARIOS must be sorted: {SCENARIOS:?}"
        );
        assert_eq!(stems(&root), SCENARIOS);
        assert!(COMMON.is_sorted(), "COMMON must be sorted: {COMMON:?}");
        let common: Vec<String> = stems(&root.join("common"))
            .into_iter()
            .map(|s| format!("{s}.jsonl"))
            .collect();
        assert_eq!(common, COMMON);
        for name in SCENARIOS {
            assert_eq!(
                text(name).unwrap(),
                std::fs::read_to_string(scenario_path(name)).unwrap()
            );
        }
    }

    #[test]
    fn bin_path_finds_the_profile_folder() {
        let unix = Path::new("/w/target/debug/deps/acp-1a2b");
        assert_eq!(
            bin_path_from(None, unix, ""),
            Path::new("/w/target/debug/fake_agent")
        );
        let plain = Path::new("/w/target/release/host");
        assert_eq!(
            bin_path_from(None, plain, ""),
            Path::new("/w/target/release/fake_agent")
        );
        let windows = Path::new("C:/w/target/debug/deps/acp-1a2b.exe");
        assert_eq!(
            bin_path_from(None, windows, ".exe"),
            Path::new("C:/w/target/debug/fake_agent.exe")
        );
        let set = Some(OsString::from("/opt/fake_agent"));
        assert_eq!(bin_path_from(set, unix, ""), Path::new("/opt/fake_agent"));
        assert_eq!(
            bin_path_from(Some(OsString::new()), unix, ""),
            Path::new("/w/target/debug/fake_agent")
        );
    }
}
