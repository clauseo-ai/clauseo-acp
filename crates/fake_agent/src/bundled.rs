//! The compiled-in scenarios, and the paths other crates' tests use.

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
        "auth-required",
        "cancel-late-updates",
        "cancel-unanswered",
        "elicitation-form",
        "elicitation-url",
        "exit-mid-turn",
        "files-multi-turn",
        "files-one-turn",
        "grandchild",
        "idle",
        "ignore-stdin-close",
        "initialize-caps",
        "limit-codex-error",
        "limit-rejected",
        "limit-warning",
        "load-replay",
        "malformed-line",
        "notice-compaction",
        "permission-cancelled",
        "permission-kinds",
        "plan",
        "prompt-stream",
        "queue-two-prompts",
        "send-now",
        "session-failure",
        "session-state",
        "steer-fallback",
        "steer-injected",
        "steer-prompt-required",
        "tool-calls",
        "unknown-update",
        "usage",
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
#[must_use]
pub fn scenario_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scenarios")
        .join(format!("{name}.jsonl"))
}

/// The `fake_agent` binary for another crate's tests: `CLAUSEO_FAKE_AGENT_BIN` if set, else
/// `fake_agent` in the target profile folder of the running test binary.
///
/// Cargo does not build a dependency's binaries for `cargo nextest run -p <other>`, so a missing
/// profile binary's error says to run `cargo build -p clauseo-fake_agent`. A missing file named by
/// the variable gets an error that names the variable instead.
///
/// # Errors
///
/// [`Error::BinaryVarNotFound`] if `CLAUSEO_FAKE_AGENT_BIN` names no file,
/// [`Error::CurrentExe`] if the running executable cannot be found, and
/// [`Error::BinaryNotFound`] if the profile folder has no `fake_agent`.
pub fn bin_path() -> Result<PathBuf, Error> {
    if let Some(var) = std::env::var_os(BIN_VAR).filter(|v| !v.is_empty()) {
        return existing(PathBuf::from(var), Source::Var);
    }
    let exe = std::env::current_exe().map_err(|source| Error::CurrentExe { source })?;
    existing(
        profile_bin(&exe, std::env::consts::EXE_SUFFIX),
        Source::Profile,
    )
}

/// The variable that overrides where [`bin_path`] looks.
const BIN_VAR: &str = "CLAUSEO_FAKE_AGENT_BIN";

/// Where a candidate binary path came from, so a missing file gets the right advice.
#[derive(Debug, Clone, Copy)]
enum Source {
    /// `CLAUSEO_FAKE_AGENT_BIN`.
    Var,
    /// The target profile folder of the running test binary.
    Profile,
}

/// `path` if it is a file, else the error that fits where it came from.
fn existing(path: PathBuf, source: Source) -> Result<PathBuf, Error> {
    match (path.is_file(), source) {
        (true, _) => Ok(path),
        (false, Source::Var) => Err(Error::BinaryVarNotFound { path }),
        (false, Source::Profile) => Err(Error::BinaryNotFound { path }),
    }
}

/// `fake_agent` in the target profile folder of executable `exe`, with the platform's
/// executable suffix. A test binary lives in `target/<profile>/deps/`.
fn profile_bin(exe: &Path, suffix: &str) -> PathBuf {
    let mut folder = exe.parent().unwrap_or_else(|| Path::new("")).to_owned();
    if folder.file_name().is_some_and(|name| name == "deps") {
        folder.pop();
    }
    folder.join(format!("fake_agent{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Reply, Scenario, StepKind, Wait};

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
    fn auth_required_code_matches_sdk() {
        let expected = i64::from(i32::from(agent_client_protocol::ErrorCode::AuthRequired));
        let scenario = Scenario::parse("auth-required", text("auth-required").unwrap()).unwrap();
        let session_new: Vec<&str> = scenario
            .steps
            .iter()
            .filter_map(|step| match &step.kind {
                StepKind::Wait(Wait::Expect(expect)) if expect.method == "session/new" => {
                    expect.label.as_deref()
                }
                _ => None,
            })
            .collect();
        let codes: Vec<i64> = scenario
            .steps
            .iter()
            .filter_map(|step| match &step.kind {
                StepKind::Respond {
                    to,
                    reply: Reply::Error(error),
                } if session_new.contains(&to.as_str()) => error["code"].as_i64(),
                _ => None,
            })
            .collect();
        assert_eq!(codes, [expected], "the auth-required error to session/new");
    }

    #[test]
    fn bin_path_finds_the_profile_folder() {
        let unix = Path::new("/w/target/debug/deps/acp-1a2b");
        assert_eq!(
            profile_bin(unix, ""),
            Path::new("/w/target/debug/fake_agent")
        );
        let plain = Path::new("/w/target/release/host");
        assert_eq!(
            profile_bin(plain, ""),
            Path::new("/w/target/release/fake_agent")
        );
        let windows = Path::new("C:/w/target/debug/deps/acp-1a2b.exe");
        assert_eq!(
            profile_bin(windows, ".exe"),
            Path::new("C:/w/target/debug/fake_agent.exe")
        );
    }

    #[test]
    fn a_missing_binary_names_where_it_came_from() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-fake_agent");
        let from_var = existing(missing.clone(), Source::Var)
            .unwrap_err()
            .to_string();
        assert_eq!(
            from_var,
            format!(
                "{}: no such file, named by CLAUSEO_FAKE_AGENT_BIN",
                missing.display()
            )
        );
        let from_profile = existing(missing.clone(), Source::Profile)
            .unwrap_err()
            .to_string();
        assert_eq!(
            from_profile,
            format!(
                "{}: no such file; run `cargo build -p clauseo-fake_agent`",
                missing.display()
            )
        );
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert_eq!(existing(manifest.clone(), Source::Var).unwrap(), manifest);
    }
}
