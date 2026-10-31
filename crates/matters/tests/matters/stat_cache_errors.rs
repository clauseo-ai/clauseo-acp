//! A stat cache (`history.git/index`) that exists but cannot be opened is an error when the
//! matter opens, not a missing cache rebuilt by reading every file of the folder again.

use clauseo_matters::Error;

use crate::fixture::Fixture;
use crate::support;

/// The index is a symlink to itself, so opening it fails with `ELOOP`: the matter does not open.
#[cfg(unix)]
#[test]
fn a_stat_cache_that_cannot_be_opened_fails_the_open() {
    let fx = Fixture::new("stat-cache-loop");
    fx.write("brief.txt", b"Heads of terms.\n");
    drop(fx.import());
    let index = fx.git_dir().join("index");
    support::remove_file(&index);
    support::symlink(&index, &index);

    let opened = fx.open(&fx.folder);
    assert!(
        matches!(&opened, Err(Error::Io { op: "open", path, .. }) if *path == index),
        "{:?}",
        opened.err()
    );
}
