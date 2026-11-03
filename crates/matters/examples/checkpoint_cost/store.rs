//! The object store on disk: the loose objects and the sizes of `objects/` and `history.git`.

use std::fs;
use std::io;
use std::path::Path;

/// The object store on disk: the loose objects (count and bytes), `objects/` and `history.git`.
#[derive(Default)]
pub(super) struct Store {
    pub(super) loose: u64,
    pub(super) loose_bytes: u64,
    pub(super) objects_bytes: u64,
    pub(super) git_dir_bytes: u64,
}

/// The bytes of every file under `dir`.
fn total(dir: &Path) -> io::Result<u64> {
    let mut sum = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            sum += total(&entry.path())?;
        } else {
            sum += entry.metadata()?.len();
        }
    }
    Ok(sum)
}

/// The loose objects under `objects/`: the files in its two-character folders, as (count, bytes).
fn loose(objects: &Path) -> io::Result<(u64, u64)> {
    let (mut count, mut bytes) = (0, 0);
    for folder in fs::read_dir(objects)? {
        let folder = folder?;
        if !folder.file_type()?.is_dir() || folder.file_name().len() != 2 {
            continue;
        }
        for object in fs::read_dir(folder.path())? {
            let object = object?;
            count += 1;
            bytes += object.metadata()?.len();
        }
    }
    Ok((count, bytes))
}

/// Measures the git dir `git_dir` of a matter's history.
pub(super) fn measure(git_dir: &Path) -> io::Result<Store> {
    let objects = git_dir.join("objects");
    let (loose, loose_bytes) = loose(&objects)?;
    Ok(Store {
        loose,
        loose_bytes,
        objects_bytes: total(&objects)?,
        git_dir_bytes: total(git_dir)?,
    })
}
