//! The std calls that change files, as thin wrappers and nothing else: the one place in
//! `fake_agent` (outside its tests) that may call them. Workspace `clippy.toml` bans each of them
//! everywhere else, and the expect below covers only this file, so the rest of `fake_agent` stays
//! under every other ban. Each wrapper does exactly what its std call does; the scenario's file
//! steps decide which paths may change, in `run/effects.rs`.

#![expect(
    clippy::disallowed_methods,
    reason = "fake_agent's file steps call std's writes here alone; clippy.toml bans them elsewhere"
)]

use std::io;
use std::path::Path;

/// `fs::write`.
pub(crate) fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    std::fs::write(path, contents)
}

/// `fs::remove_file`.
pub(crate) fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// `fs::rename`.
pub(crate) fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Appends `text` to `path`, creating it (`OpenOptions` with create and append).
pub(crate) fn append(path: impl AsRef<Path>, text: &str) -> io::Result<()> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(text.as_bytes())
}

/// Writes `bytes` over `path` in place: the same file, its links kept, truncated first
/// (`OpenOptions` with write and truncate).
pub(crate) fn write_in_place(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?;
    file.write_all(bytes)
}
