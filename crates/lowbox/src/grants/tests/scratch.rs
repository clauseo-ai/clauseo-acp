//! The std calls that change files, as thin wrappers and nothing else: the one place the grants'
//! tests make and remove their folders, links and junctions. Workspace `clippy.toml` bans them,
//! tests included, and the expect below covers only this file.
#![expect(
    clippy::disallowed_methods,
    reason = "the grants' tests change their own scratch folders here alone"
)]

use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

/// `fs::create_dir_all`.
pub(super) fn create_dir_all(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
}

/// `fs::write`.
pub(super) fn write(path: &Path, contents: &[u8]) {
    std::fs::write(path, contents).unwrap();
}

/// `fs::remove_dir_all`, when it exists. A junction or link inside is removed, not followed.
pub(super) fn remove_all(path: &Path) {
    if path.exists() {
        let _left_behind = std::fs::remove_dir_all(path);
    }
}

/// A symbolic link `link` to the folder `target`.
#[cfg(unix)]
pub(super) fn symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// A junction `link` to the folder `target` (`mklink /J` needs no privilege).
#[cfg(windows)]
pub(super) fn junction(target: &Path, link: &Path) {
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .status()
        .unwrap();
    assert!(status.success(), "mklink /J failed");
}

/// The short (8.3) name of `path`, from `cmd`'s `%~s`; `None` when the volume makes none.
#[cfg(windows)]
pub(super) fn short_name(path: &Path) -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("cmd")
        .raw_arg(format!(
            r#"/C for %I in ("{}") do @echo %~sI"#,
            path.display()
        ))
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let short = PathBuf::from(text.trim());
    (short.as_os_str() != path.as_os_str() && !text.trim().is_empty()).then_some(short)
}
