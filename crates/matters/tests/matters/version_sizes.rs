//! The sizes a version lists: each file's size in that version, whatever the folder holds now
//! and whether or not the stat cache survived.

use crate::fixture::{Fixture, cid, created, meta, tools};
use crate::support;

/// Each file of checkpoint `n` with its size, by path.
fn sizes(matter: &clauseo_matters::Matter, n: u64) -> Vec<(String, u64)> {
    matter
        .files_at(cid(n))
        .unwrap()
        .into_iter()
        .map(|file| (file.path.as_str().to_owned(), file.size))
        .collect()
}

/// An older version lists each file at the size it had then, not the size it has now.
#[test]
fn an_older_version_lists_the_sizes_it_had() {
    let fx = Fixture::new("sizes-older");
    fx.write("a.txt", "one");
    let mut matter = fx.import();
    fx.write("a.txt", "a good deal longer now");
    created(matter.after_turn(&meta(2), &tools(&["a.txt"])).unwrap(), 2);

    assert_eq!(sizes(&matter, 1), [("a.txt".to_owned(), 3)]);
    assert_eq!(sizes(&matter, 2), [("a.txt".to_owned(), 22)]);
}

/// After the stat cache is lost, a reopened matter still lists each file's real size.
#[test]
fn sizes_survive_a_lost_stat_cache() {
    let fx = Fixture::new("sizes-lost-cache");
    fx.write("a.txt", "one");
    fx.write("sub/b.txt", "bee bee");
    drop(fx.import());

    support::remove_file(fx.git_dir().join("index"));
    let matter = fx.open(&fx.folder).unwrap();
    assert_eq!(
        sizes(&matter, 1),
        [("a.txt".to_owned(), 3), ("sub/b.txt".to_owned(), 7)]
    );
}
