//! What a crash left in the folder of a matter, cleared as the host's matter worker opens it
//! (the matter's one writer, before any job). On every OS: the temp files of an unfinished write.
//! On Windows also the backups of a crashed rewrite (`matter/backups.rs`), the one OS that makes
//! them. `Matter::open` itself reads nothing in the folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::Matter;
use crate::checkpoint::Core;
use crate::write;

/// Clears what a crash left in the folder of `matter`, from the open of the host's matter worker
/// (the matter's one writer, before any job, so no write of its own is in flight): on every OS
/// the temp files of an unfinished write (exactly `.clauseo-<digits>.tmp`, regular files, in the
/// matter folder and every folder the stat cache has a file in, [`write::sweep_temps`]), and on
/// Windows the backups (`recover_backups`). A temp file that cannot be removed (held open by
/// another process on Windows) stays, tried once and logged as a count; nothing here fails the
/// open.
///
/// A cached folder that is now a symlink or a junction (or any folder on the way to it, below the
/// matter folder) is skipped: the sweep never reaches outside the matter through a link. The
/// check comes before the folder is listed, so a link swapped in between the two is still
/// followed for that one listing.
pub fn recover_leftovers(matter: &mut Matter) {
    for dir in folders(&matter.core, &matter.folder).keys() {
        if plain_folders(&matter.folder, dir) {
            write::sweep_temps(dir);
        }
    }
    #[cfg(windows)]
    super::backups::recover_backups(matter);
}

/// Whether every folder from below `folder` down to `dir` is a plain folder: not a symlink or a
/// junction (std's `is_dir` on the link's own metadata), and there. `folder` itself is not
/// checked: it is the matter, as the lawyer chose it. std treats only name-surrogate reparse
/// points (symlinks, junctions) as links, so a `OneDrive` or other cloud-tagged folder is still
/// swept.
fn plain_folders(folder: &Path, dir: &Path) -> bool {
    let Ok(rest) = dir.strip_prefix(folder) else {
        return false;
    };
    let mut at = folder.to_owned();
    rest.components().all(|part| {
        at.push(part);
        std::fs::symlink_metadata(&at).is_ok_and(|metadata| metadata.is_dir())
    })
}

/// The matter folder and every folder the stat cache has a file in, each with the names the
/// cache holds in it.
pub(super) fn folders(core: &Core, folder: &Path) -> BTreeMap<PathBuf, Vec<String>> {
    let mut dirs: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    dirs.insert(folder.to_owned(), Vec::new());
    for entry in &core.cache.entries {
        let (dir, name) = match entry.path.as_str().rsplit_once('/') {
            Some((parent, name)) => (folder.join(parent), name),
            None => (folder.to_owned(), entry.path.as_str()),
        };
        dirs.entry(dir).or_default().push(name.to_owned());
    }
    dirs
}

#[cfg(test)]
mod tests {
    /// Windows: a folder that is a reparse point whose tag is no name surrogate (as `OneDrive`'s
    /// cloud tags are not) counts as plain, so the sweep still lists it. Only the check: a temp
    /// cannot be put inside, since with no filter for the tag the folder cannot be entered, and
    /// only an empty folder takes a reparse point. Skipped where the volume refuses the tag.
    #[cfg(windows)]
    #[test]
    fn a_folder_with_a_reparse_tag_that_is_no_link_counts_as_plain_on_windows() {
        use crate::test_dir::TestDir;
        use crate::write::{create_dir, create_dir_all, junction, set_foreign_reparse_tag};
        let dir = TestDir::new("leftovers-cloud-tag");
        let folder = dir.path().join("matter");
        let cloudy = folder.join("Papers");
        create_dir_all(&cloudy).unwrap();
        if let Err(err) = set_foreign_reparse_tag(&cloudy, 0x0000_7E57) {
            println!("skipped: no reparse point of our own could be set here ({err})");
            return;
        }
        let kind = std::fs::symlink_metadata(&cloudy).unwrap().file_type();
        assert!(!kind.is_symlink(), "{kind:?}");
        assert!(super::plain_folders(&folder, &cloudy));
        // A junction beside it still does not.
        let target = dir.path().join("outside");
        create_dir(&target).unwrap();
        let linked = folder.join("Linked");
        junction(&target, &linked);
        assert!(!super::plain_folders(&folder, &linked));
    }
}
