//! The short form beside the long: added once and after its path, never for a volume that makes
//! none (the call gives the path back) or a path with none, never asked of a network path.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use super::with_short_forms_by;
use crate::network_paths::NetEnv;

fn abs(path: &str) -> PathBuf {
    PathBuf::from(crate::testing::abs(path))
}

#[test]
fn a_short_form_goes_after_its_path_once() {
    let net = NetEnv::current();
    let long = abs("/Users/clauseo-lawyer/Matters/B");
    let short = abs("/Users/CLAUSE~1/Matters/B");
    let given = short.clone();
    let found = with_short_forms_by(vec![long.clone(), short.clone()], &net, &|path: &Path| {
        (path == long).then(|| given.clone())
    });
    assert_eq!(found, [long, short]);
}

/// A volume with short names off gives the path back as it is: nothing is added.
#[test]
fn a_volume_without_short_names_adds_nothing() {
    let net = NetEnv::current();
    let paths = vec![
        abs("/Users/l/Matters/B"),
        abs("/Users/l/AppData/Local/Clauseo"),
    ];
    let same = with_short_forms_by(paths.clone(), &net, &|path: &Path| Some(path.to_path_buf()));
    assert_eq!(same, paths);
    let none = with_short_forms_by(paths.clone(), &net, &|_: &Path| None);
    assert_eq!(none, paths);
}

#[test]
fn a_network_path_is_never_asked() {
    let net = NetEnv::current();
    let asked = Cell::new(0);
    let share = PathBuf::from(r"\\server\share\Matters\B");
    let net = NetEnv {
        windows: true,
        ..net
    };
    let found = with_short_forms_by(vec![share.clone()], &net, &|_: &Path| {
        asked.set(asked.get() + 1);
        None
    });
    assert_eq!(found, [share]);
    assert_eq!(asked.get(), 0);
}
