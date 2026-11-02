//! The pre-write guard's fast path: `main` asks [`guard_requested`] before anything else starts,
//! and on yes runs `clauseo_host::pre_write_guard::run` with the remaining arguments on stdin
//! and stderr and exits with its code: no Tauri, no logging, no settings read. Claude Code's
//! `PreToolUse` hook starts the app this way before each file edit and shell command
//! (`HostConfig::with_pre_write_guard`).

use std::ffi::OsStr;

/// Whether the first argument after the program, `first`, is exactly
/// `clauseo_host::pre_write_guard::FLAG`.
pub fn guard_requested(first: Option<&OsStr>) -> bool {
    first == Some(OsStr::new(clauseo_host::pre_write_guard::FLAG))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::guard_requested;

    #[test]
    fn only_the_exact_flag_first_runs_the_guard() {
        assert!(guard_requested(Some(OsStr::new("--pre-write-guard"))));
        for other in [
            "--pre-write-guard=1",
            "--Pre-Write-Guard",
            " --pre-write-guard",
            "pre-write-guard",
            "",
            "clauseo://open",
        ] {
            assert!(!guard_requested(Some(OsStr::new(other))), "{other:?}");
        }
        assert!(!guard_requested(None));
    }
}
