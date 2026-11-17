//! The short form beside the long: added once and after its path, never for a volume that makes
//! none (the call gives the path back), never asked of a network path; a path that can't be
//! read is counted for the warning, a missing one (a matter whose folder is gone) is not.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use super::{Looked, short_forms_counted};
use crate::network_paths::NetEnv;

fn abs(path: &str) -> PathBuf {
    PathBuf::from(crate::testing::abs(path))
}

/// This system's rules with no remote mounts: a path counts as network by its text alone, never
/// by the mount table of the machine the tests run on (where an unlisted root file system type
/// makes every path remote).
fn local_net() -> NetEnv {
    NetEnv {
        remote_mounts: Vec::new(),
        ..NetEnv::current()
    }
}

#[test]
fn a_short_form_goes_after_its_path_once() {
    let net = local_net();
    let long = abs("/Users/clauseo-lawyer/Matters/B");
    let short = abs("/Users/CLAUSE~1/Matters/B");
    let given = short.clone();
    let (found, unread) =
        short_forms_counted(vec![long.clone(), short.clone()], &net, &|path: &Path| {
            if path == long {
                Looked::Form(given.clone())
            } else {
                Looked::Form(path.to_path_buf())
            }
        });
    assert_eq!(found, [long, short]);
    assert_eq!(unread, 0);
}

/// A volume with short names off gives the path back as it is: nothing is added.
#[test]
fn a_volume_without_short_names_adds_nothing() {
    let net = local_net();
    let paths = vec![
        abs("/Users/l/Matters/B"),
        abs("/Users/l/AppData/Local/Clauseo"),
    ];
    let (same, unread) = short_forms_counted(paths.clone(), &net, &|path: &Path| {
        Looked::Form(path.to_path_buf())
    });
    assert_eq!(same, paths);
    assert_eq!(unread, 0);
}

/// A folder that is gone adds nothing and is no failure; one that exists and can't be read is
/// counted for the warning.
#[test]
fn a_missing_folder_is_not_counted_and_an_unread_one_is() {
    let net = local_net();
    let (gone, there) = (abs("/Users/l/Matters/Gone"), abs("/Users/l/Matters/B"));
    let (found, unread) =
        short_forms_counted(vec![gone.clone(), there.clone()], &net, &|path: &Path| {
            if path == gone {
                Looked::Missing
            } else {
                Looked::Unread
            }
        });
    assert_eq!(found, [gone, there]);
    assert_eq!(unread, 1);
}

#[test]
fn a_network_path_is_never_asked() {
    let net = local_net();
    let asked = Cell::new(0);
    let share = PathBuf::from(r"\\server\share\Matters\B");
    let net = NetEnv {
        windows: true,
        ..net
    };
    let (found, unread) = short_forms_counted(vec![share.clone()], &net, &|_: &Path| {
        asked.set(asked.get() + 1);
        Looked::Unread
    });
    assert_eq!(found, [share]);
    assert_eq!((asked.get(), unread), (0, 0));
}
