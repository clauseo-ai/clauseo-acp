//! Helpers this crate's unit tests share.

/// `path`, written `/a/b`, as an absolute path on this OS: `C:\a\b` on Windows. Settings
/// refuses a server command that isn't absolute, and `/opt/homebrew/bin/npx` isn't on Windows,
/// so a test's command goes through here.
pub(crate) fn abs(path: &str) -> String {
    if cfg!(windows) {
        format!("C:{}", path.replace('/', "\\"))
    } else {
        path.to_owned()
    }
}
