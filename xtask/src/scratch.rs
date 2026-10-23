//! Scratch directories for tests that touch real files, for any check or command in xtask. Each
//! lives under the temp dir and is removed when it drops, so a test that fails half-way leaves
//! nothing behind.
//!
//! xtask's tests change files only through the helpers here (`Scratch::write`,
//! `Scratch::create_dir_all`, ...): workspace `clippy.toml` bans the std calls everywhere else, in
//! tests too, and `sys.rs` holds the only calls to them. Each helper does what the std call of the
//! same name does, and panics when the path it changes is not inside a scratch directory.

mod sys;

use std::fs::FileTimes;
#[cfg(unix)]
use std::fs::{File, Permissions};
use std::io;
use std::ops::Deref;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The start of every scratch directory's name.
const PREFIX: &str = "xtask-";

/// Numbers the scratch directories of this process, so one name asked for twice at once (a
/// planted case run by its own test and by the registry test, on two threads) gets two.
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// An empty directory `xtask-<name>-<pid>-<n>` under the temp dir, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// Makes the directory, clearing what a killed earlier run left under the same name.
    ///
    /// Start `name` with the module, e.g. `buildtime-stray`, so a directory left behind says
    /// which test made it. The pid and a per-process number keep two directories apart.
    pub(crate) fn new(name: &str) -> Scratch {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("{PREFIX}{name}-{}-{n}", std::process::id()));
        if dir.exists() {
            sys::remove_dir_all(&dir).expect("clear the old scratch dir");
        }
        sys::create_dir_all(&dir).expect("create the scratch dir");
        Scratch(dir)
    }

    /// `fs::write` of `contents` to `path`.
    pub(crate) fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
        sys::write(inside(path.as_ref()), contents.as_ref())
    }

    /// `fs::create_dir` of `path`: fails when it exists or its parent is missing.
    pub(crate) fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
        sys::create_dir(inside(path.as_ref()))
    }

    /// `fs::create_dir_all` of `path`, with every missing parent.
    pub(crate) fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        sys::create_dir_all(inside(path.as_ref()))
    }

    /// `fs::remove_file` of `path`.
    pub(crate) fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        sys::remove_file(inside(path.as_ref()))
    }

    /// `fs::set_times` of `path`.
    pub(crate) fn set_times(path: impl AsRef<Path>, times: FileTimes) -> io::Result<()> {
        sys::set_times(inside(path.as_ref()), times)
    }

    /// `fs::copy` of `from` (which may lie anywhere) to `to`.
    pub(crate) fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        sys::copy(from.as_ref(), inside(to.as_ref()))
    }

    /// `fs::set_permissions` of `path`.
    #[cfg(unix)]
    pub(crate) fn set_permissions(
        path: impl AsRef<Path>,
        permissions: Permissions,
    ) -> io::Result<()> {
        sys::set_permissions(inside(path.as_ref()), permissions)
    }

    /// Opens `path` for writing without creating it, to test whether it can be written.
    #[cfg(unix)]
    pub(crate) fn open_for_writing(path: impl AsRef<Path>) -> io::Result<File> {
        sys::open_for_writing(inside(path.as_ref()))
    }

    /// A symlink at `link` to `target` (which may be relative to the link).
    #[cfg(unix)]
    pub(crate) fn symlink(target: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
        sys::symlink(target.as_ref(), inside(link.as_ref()))
    }

    /// A symlink at `link` to the directory `target`: `symlink` on Unix, `symlink_dir` on
    /// Windows. On Windows a relative `target` written with `/` (`../../elsewhere`) is stored
    /// with `\`, since Windows stores the target as given and resolves a relative one only
    /// with `\`.
    pub(crate) fn symlink_dir(target: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
        #[cfg(unix)]
        let linked = sys::symlink(target.as_ref(), inside(link.as_ref()));
        #[cfg(windows)]
        let linked = sys::symlink_dir(
            &target.as_ref().components().collect::<PathBuf>(),
            inside(link.as_ref()),
        );
        linked
    }
}

/// `path`, once it is checked to lie in a scratch directory: under the temp dir, in a folder
/// whose name starts with `xtask-`. The check follows symlinks: `path` itself must not be one,
/// and `path`, or its deepest folder that exists, resolved with `dunce::canonicalize`, must lie
/// in a scratch dir, so no link made in a scratch dir leads a write out of it. Panics
/// otherwise, so a test never changes a file outside one.
fn inside(path: &Path) -> &Path {
    let outside = || {
        format!(
            "{} is not inside a scratch directory; make one with Scratch::new",
            path.display()
        )
    };
    assert!(
        path.is_absolute() && !path.components().any(|part| part == Component::ParentDir),
        "{}",
        outside()
    );
    assert!(
        !path.is_symlink(),
        "{} is a symlink; Scratch never writes through one",
        path.display()
    );
    let mut folder = Some(path);
    while let Some(dir) = folder.filter(|dir| !dir.exists()) {
        folder = dir.parent();
    }
    let resolved = folder.and_then(|dir| dunce::canonicalize(dir).ok());
    let temp = dunce::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
    let first = resolved
        .as_deref()
        .and_then(|dir| dir.strip_prefix(&temp).ok())
        .and_then(|rest| rest.components().next());
    let in_scratch = matches!(
        first,
        Some(Component::Normal(name)) if name.to_string_lossy().starts_with(PREFIX)
    );
    assert!(in_scratch, "{}", outside());
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
        if let Err(error) = sys::remove_dir_all(&self.0) {
            eprintln!("could not remove {}: {error}", self.0.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    #[cfg(unix)]
    #[test]
    fn a_symlink_in_a_scratch_dir_leads_no_write_out_of_it() {
        let dir = Scratch::new("scratch-symlink");
        let elsewhere = Scratch::new("scratch-symlink-target");
        // A folder link pointing out of every scratch dir, and a file link to one in another.
        Scratch::symlink_dir(std::env::temp_dir(), dir.join("out")).expect("a folder link");
        Scratch::write(elsewhere.join("f.txt"), "kept").expect("a file elsewhere");
        Scratch::symlink(elsewhere.join("f.txt"), dir.join("f.txt")).expect("a file link");

        let through = dir.join("out/escaped.txt");
        assert_eq!(
            refusal(&through),
            format!(
                "{} is not inside a scratch directory; make one with Scratch::new",
                through.display()
            )
        );
        let nested = dir.join("out/new/deeper.txt");
        assert_eq!(
            refusal(&nested),
            format!(
                "{} is not inside a scratch directory; make one with Scratch::new",
                nested.display()
            )
        );
        assert_eq!(
            refusal(&dir.join("f.txt")),
            format!(
                "{} is a symlink; Scratch never writes through one",
                dir.join("f.txt").display()
            )
        );
        assert!(!std::env::temp_dir().join("escaped.txt").exists());
        assert_eq!(
            fs::read_to_string(elsewhere.join("f.txt")).expect("read it"),
            "kept"
        );
    }
}
