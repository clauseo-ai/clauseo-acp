//! The `SessionStart` settings canary: the guard program's second mode, run by Claude Code's
//! `SessionStart` hook that the host puts in a Claude session's flag settings next to the guard's
//! `PreToolUse` entry (`agent/loads/session_canary.rs`).
//!
//! Claude Code 2.1.293 runs its `SessionStart` hooks, and finishes them, before claude-agent-acp
//! 0.89.1 answers `session/new`, `session/load` or `session/resume`, so a canary that ran proves
//! the flag settings (`--settings`) loaded; the host reads the nonce back after the answer,
//! before any prompt (`actor/turn/session/canary.rs`).
//!
//! `--pre-write-guard --session-canary <file> <nonce>`: writes `<nonce>` to the new file
//! `<file>` (an absolute path; atomic, never over anything already there,
//! `write::write_new_private_file`) and exits 0. Anything else (other arguments, a nonce that is
//! not 1 to [`MAX_NONCE`] ASCII letters and digits, a failed write) exits [`super::DENY`] with
//! one line on stderr, and the host, finding no nonce, refuses the session. The hook's input on
//! stdin is never read: the response to the session call waits for this process, so it does the
//! least it can.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::Path;

/// The argument after `--pre-write-guard` that runs the canary instead of the guard.
pub(super) const FLAG: &str = "--session-canary";

/// The longest nonce the canary writes (the host's are 32 hex digits).
const MAX_NONCE: usize = 128;

/// `Some(exit code)` when `args` (the arguments after `--pre-write-guard`) start with [`FLAG`]:
/// 0 once the nonce is written, [`super::DENY`] with one stderr line otherwise. `None` when they
/// do not, and the guard runs.
pub(super) fn run(args: &[OsString]) -> Option<i32> {
    let (flag, rest) = args.split_first()?;
    if flag.as_os_str() != FLAG {
        return None;
    }
    let code = match write_nonce(rest) {
        Ok(()) => 0,
        Err(why) => {
            let line = format!("Clauseo's settings canary: {why}\n");
            std::io::stderr()
                .lock()
                .write_all(line.as_bytes())
                .unwrap_or_default();
            super::DENY
        }
    };
    Some(code)
}

/// Writes the nonce of `rest` (`<file> <nonce>`) to its file, or says why not.
fn write_nonce(rest: &[OsString]) -> Result<(), String> {
    let [file, nonce] = rest else {
        return Err(format!(
            "expected a nonce file and a nonce, got {} arguments",
            rest.len()
        ));
    };
    let file = Path::new(file);
    if !file.is_absolute() {
        return Err("the nonce file is not an absolute path".to_owned());
    }
    let nonce = nonce
        .to_str()
        .filter(|nonce| valid_nonce(nonce))
        .ok_or_else(|| format!("the nonce is not 1 to {MAX_NONCE} ASCII letters and digits"))?;
    crate::write::write_new_private_file(file, nonce.as_bytes())
        .map_err(|error| format!("the nonce file was not written: {error}"))
}

/// Whether `nonce` is 1 to [`MAX_NONCE`] ASCII letters and digits.
fn valid_nonce(nonce: &str) -> bool {
    !nonce.is_empty()
        && nonce.len() <= MAX_NONCE
        && nonce.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;

    use super::{FLAG, run};
    use crate::pipeline::tests::support::TempDir;

    /// `args` as the guard gets them.
    fn args(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// The canary's arguments for `file` and `nonce`.
    fn canary(file: &Path, nonce: &str) -> Vec<OsString> {
        vec![FLAG.into(), file.into(), nonce.into()]
    }

    #[test]
    fn other_arguments_leave_the_guard_to_run() {
        assert_eq!(run(&[]), None);
        assert_eq!(run(&args(&["--matter", "/m"])), None);
        assert_eq!(run(&args(&["--ask-all", FLAG, "/x", "n"])), None);
    }

    #[test]
    fn the_canary_writes_its_nonce_to_a_new_file_and_exits_0() {
        let dir = TempDir::new("canary-writes");
        let file = dir.path().join("canary-1");
        assert_eq!(run(&canary(&file, "0123abcdef")), Some(0));
        assert_eq!(std::fs::read(&file).unwrap(), b"0123abcdef");
    }

    #[test]
    fn a_file_already_there_is_never_written_over() {
        let dir = TempDir::new("canary-taken");
        let file = dir.path().join("canary-1");
        crate::write::write_file(&file, b"earlier").unwrap();
        assert_eq!(run(&canary(&file, "abc")), Some(super::super::DENY));
        assert_eq!(std::fs::read(&file).unwrap(), b"earlier");
    }

    #[test]
    fn bad_arguments_refuse_and_write_nothing() {
        let dir = TempDir::new("canary-bad");
        let file = dir.path().join("canary-1");
        let deny = Some(super::super::DENY);
        let text = file.to_str().unwrap();
        assert_eq!(run(&args(&[FLAG])), deny);
        assert_eq!(run(&args(&[FLAG, text])), deny);
        assert_eq!(run(&args(&[FLAG, text, "abc", "extra"])), deny);
        assert_eq!(run(&args(&[FLAG, text, ""])), deny);
        assert_eq!(run(&args(&[FLAG, text, "not a nonce"])), deny);
        let long = "a".repeat(129);
        assert_eq!(run(&args(&[FLAG, text, long.as_str()])), deny);
        assert_eq!(run(&args(&[FLAG, "canary-1", "abc"])), deny);
        assert!(!file.exists());
        assert!(!Path::new("canary-1").exists());
    }

    #[test]
    fn a_missing_folder_refuses() {
        let dir = TempDir::new("canary-no-folder");
        let file = dir.path().join("gone").join("canary-1");
        assert_eq!(run(&canary(&file, "abc")), Some(super::super::DENY));
    }
}
