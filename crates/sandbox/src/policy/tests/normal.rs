//! Normalisation before the comparisons: a Windows verbatim drive or UNC path is its plain form,
//! and on macOS the data volume's spelling is `/`. The pure helpers run on every OS; the
//! comparisons run where their form is the platform's.

use super::*;
use crate::policy::compare::{strip_data_volume, strip_verbatim};

#[test]
fn a_verbatim_drive_or_unc_path_is_its_plain_form() {
    assert_eq!(
        strip_verbatim(Path::new(r"\\?\C:\Users\x")),
        Some(PathBuf::from(r"C:\Users\x"))
    );
    assert_eq!(
        strip_verbatim(Path::new(r"\\?\UNC\server\share\x")),
        Some(PathBuf::from(r"\\server\share\x"))
    );
    // Not a drive or a UNC share: kept as it is.
    assert_eq!(strip_verbatim(Path::new(r"C:\Users\x")), None);
    assert_eq!(strip_verbatim(Path::new(r"\\?\Volume{1234}\x")), None);
    assert_eq!(strip_verbatim(Path::new(r"\\server\share\x")), None);
}

#[test]
fn the_data_volume_spelling_is_the_plain_path() {
    assert_eq!(
        strip_data_volume(Path::new("/System/Volumes/Data/Users/x")),
        Some(PathBuf::from("/Users/x"))
    );
    assert_eq!(
        strip_data_volume(Path::new("/System/Volumes/Data")),
        Some(PathBuf::from("/"))
    );
    // A name that only starts the same is another folder.
    assert_eq!(
        strip_data_volume(Path::new("/System/Volumes/Datax/Users")),
        None
    );
    assert_eq!(strip_data_volume(Path::new("/Users/x")), None);
}

#[cfg(windows)]
#[test]
fn a_verbatim_path_compares_as_its_plain_form() {
    assert!(under(Path::new(r"\\?\C:\a\b"), Path::new(r"C:\a")));
    assert!(same(Path::new(r"\\?\C:\a"), Path::new(r"C:\a")));
    assert!(under(
        Path::new(r"\\?\UNC\srv\sh\a"),
        Path::new(r"\\srv\sh")
    ));
    assert!(!under(Path::new(r"\\?\C:\ab"), Path::new(r"C:\a")));
}

#[cfg(target_os = "macos")]
#[test]
fn the_data_volume_spelling_compares_as_the_plain_path() {
    assert!(under(
        Path::new("/System/Volumes/Data/Users/x/a"),
        Path::new("/Users/x")
    ));
    assert!(same(
        Path::new("/System/Volumes/Data/Users/x"),
        Path::new("/Users/x")
    ));
    assert!(same(
        Path::new("/Users/x"),
        Path::new("/System/Volumes/Data/Users/x")
    ));
    assert!(!under(
        Path::new("/System/Volumes/Datax/Users"),
        Path::new("/Users")
    ));
}
