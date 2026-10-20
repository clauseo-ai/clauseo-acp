//! Scratch directories for tests that touch real files, for any check or command in xtask. Each
//! lives under the temp dir and is removed when it drops, so a test that fails half-way leaves
//! nothing behind.
//!
//! xtask's tests change files only through the helpers here (`Scratch::write`,
//! `Scratch::create_dir_all`, ...): workspace `clippy.toml` bans the std calls everywhere else, in
//! tests too. Each helper does what the std call of the same name does, and panics when the path
//! it changes is not inside a scratch directory.

#![expect(
    clippy::disallowed_methods,
    reason = "xtask's tests change files only through Scratch; clippy.toml bans the std calls \
              elsewhere"
)]

use std::fs::{self, File, OpenOptions, Permissions};
use std::io;
use std::ops::Deref;
use std::path::{Component, Path, PathBuf};

/// The start of every scratch directory's name.
const PREFIX: &str = "xtask-";

/// An empty directory `xtask-<name>-<pid>` under the temp dir, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// Makes the directory, clearing what a killed earlier run left under the same name.
    ///
    /// `name` must be unique per test, across all of xtask's tests. `cargo test` runs tests as
    /// threads of one process, so two tests sharing a name would share a pid too, and each would
    /// wipe the other's directory. Start it with the module, e.g. `buildtime-stray`.
    pub(crate) fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("{PREFIX}{name}-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("clear the old scratch dir");
        }
        fs::create_dir_all(&dir).expect("create the scratch dir");
        Scratch(dir)
    }

    /// `fs::write` of `contents` to `path`.
    pub(crate) fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
        fs::write(inside(path.as_ref()), contents)
    }

    /// `fs::create_dir` of `path`: fails when it exists or its parent is missing.
    pub(crate) fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
        fs::create_dir(inside(path.as_ref()))
    }

    /// `fs::create_dir_all` of `path`, with every missing parent.
    pub(crate) fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        fs::create_dir_all(inside(path.as_ref()))
    }

    /// `fs::remove_file` of `path`.
    pub(crate) fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        fs::remove_file(inside(path.as_ref()))
    }

    /// `fs::copy` of `from` (which may lie anywhere) to `to`.
    pub(crate) fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        fs::copy(from, inside(to.as_ref()))
    }

    /// `fs::set_permissions` of `path`.
    pub(crate) fn set_permissions(
        path: impl AsRef<Path>,
        permissions: Permissions,
    ) -> io::Result<()> {
        fs::set_permissions(inside(path.as_ref()), permissions)
    }

    /// Opens `path` for writing without creating it, to test whether it can be written.
    pub(crate) fn open_for_writing(path: impl AsRef<Path>) -> io::Result<File> {
        OpenOptions::new().write(true).open(inside(path.as_ref()))
    }

    /// A symlink at `link` to `target` (which may be relative to the link).
    #[cfg(unix)]
    pub(crate) fn symlink(target: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
        std::os::unix::fs::symlink(target, inside(link.as_ref()))
    }

    /// A symlink at `link` to the directory `target`: `symlink` on Unix, `symlink_dir` on
    /// Windows.
    pub(crate) fn symlink_dir(target: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(target, inside(link.as_ref()));
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(target, inside(link.as_ref()));
        linked
    }
}

/// `path`, once it is checked to lie in a scratch directory: under the temp dir, in a folder
/// whose name starts with `xtask-`. Panics otherwise, so a test never changes a file outside one.
fn inside(path: &Path) -> &Path {
    let first = path
        .strip_prefix(std::env::temp_dir())
        .ok()
        .and_then(|rest| rest.components().next());
    let in_scratch = matches!(
        first,
        Some(Component::Normal(name)) if name.to_string_lossy().starts_with(PREFIX)
    );
    assert!(
        in_scratch && !path.components().any(|part| part == Component::ParentDir),
        "{} is not inside a scratch directory; make one with Scratch::new",
        path.display()
    );
    path
}

impl Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("could not remove {}: {error}", self.0.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panic message `inside` gives for `path`.
    fn refusal(path: &Path) -> String {
        let panic = std::panic::catch_unwind(|| inside(path).to_path_buf())
            .expect_err("the path is refused");
        panic.downcast_ref::<String>().cloned().unwrap_or_default()
    }

    #[test]
    fn helpers_change_files_only_inside_a_scratch_dir() {
        let dir = Scratch::new("scratch-inside");
        Scratch::create_dir_all(dir.join("a/b")).expect("a folder inside");
        Scratch::write(dir.join("a/b/c.txt"), "x").expect("a file inside");
        assert_eq!(
            fs::read_to_string(dir.join("a/b/c.txt")).expect("read it"),
            "x"
        );

        let outside = std::env::temp_dir().join("elsewhere/c.txt");
        assert_eq!(
            refusal(&outside),
            format!(
                "{} is not inside a scratch directory; make one with Scratch::new",
                outside.display()
            )
        );
        let escaping = dir.join("../c.txt");
        assert_eq!(
            refusal(&escaping),
            format!(
                "{} is not inside a scratch directory; make one with Scratch::new",
                escaping.display()
            )
        );
    }
}
