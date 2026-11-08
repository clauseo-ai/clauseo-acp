//! The Git Bash stub (0049, "The Git tree"): when Git Bash is off for a run, the agent's
//! `CLAUDE_CODE_GIT_BASH_PATH` names `<admin>\git\_off\bash.exe`, a copy of this runner exe
//! that set-up makes, rather than nothing. Claude Code, left without that variable or given a
//! file that isn't there, tries `C:\Program Files\Git\bin\bash.exe` on its own (2.1.318); the
//! stub keeps it from the lawyer's Git Bash.
//!
//! Claude Code takes the variable only when the file is there and named `bash.exe`, `sh.exe`,
//! `bash` or `sh`, and starts it with its own arguments. So the runner is the stub when its own
//! file is named `bash.exe` ([`is_stub`]): it says [`stub_line`], with the reason the outer
//! runner put in [`OFF_VAR`], on stderr and exits 1.

use std::path::Path;

/// The variable the outer runner gives the agent the reason Git Bash is off in, which the stub
/// says.
pub const OFF_VAR: &str = "CLAUSEO_GIT_BASH_OFF";
/// The stub's path from the admin folder.
pub const STUB: [&str; 3] = ["git", "_off", "bash.exe"];
/// The stub's exit code.
pub const STUB_EXIT: u32 = 1;
/// The most of the reason the stub says.
const REASON_CAP: usize = 300;

/// Whether the runner exe at `exe` is the stub: its file is named `bash.exe` (without case).
pub fn is_stub(exe: &Path) -> bool {
    exe.to_str()
        .and_then(|path| path.rsplit(['\\', '/']).next())
        .is_some_and(|name| name.eq_ignore_ascii_case(STUB[2]))
}

/// The stub's one line for `reason` ([`OFF_VAR`]'s value): printable characters alone, at most
/// [`REASON_CAP`] of them; `unknown` when none is left.
pub fn stub_line(reason: Option<&str>) -> String {
    let reason: String = reason
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(REASON_CAP)
        .collect();
    let reason = reason.trim();
    let reason = if reason.is_empty() { "unknown" } else { reason };
    format!("Git Bash isn't available in the Safe workspace for this session: {reason}")
}

/// The stub's run: [`stub_line`] on stderr, then [`STUB_EXIT`].
pub fn run_stub() -> u32 {
    use std::io::Write as _;
    let reason = std::env::var(OFF_VAR).ok();
    let mut stderr = std::io::stderr().lock();
    drop(writeln!(stderr, "{}", stub_line(reason.as_deref())).and_then(|()| stderr.flush()));
    STUB_EXIT
}
