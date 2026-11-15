use std::path::Path;

use super::{file_uri, inside, uri_of, within};
use crate::write;

#[test]
fn uri_keeps_unreserved_and_slashes() {
    assert_eq!(
        uri_of("/m/Brief-1_a.~txt", false),
        "file:///m/Brief-1_a.~txt"
    );
}

#[test]
fn uri_encodes_space_percent_hash_and_non_ascii() {
    assert_eq!(
        uri_of("/Scratch 2/100% #1/é.txt", false),
        "file:///Scratch%202/100%25%20%231/%C3%A9.txt"
    );
}

#[test]
fn uri_of_a_windows_drive_path() {
    assert_eq!(
        uri_of(r"C:\Matters\Scratch 2\brief.txt", true),
        "file:///C:/Matters/Scratch%202/brief.txt"
    );
    // A colon anywhere else is encoded.
    assert_eq!(uri_of("/a:b", false), "file:///a%3Ab");
}

#[test]
fn file_uri_of_this_os() {
    if cfg!(windows) {
        assert_eq!(file_uri(Path::new(r"C:\a b")), "file:///C:/a%20b");
    } else {
        assert_eq!(file_uri(Path::new("/a b")), "file:///a%20b");
    }
}

#[test]
fn inside_follows_dots_and_refuses_what_does_not_resolve() {
    let root = std::env::temp_dir().join(format!("clauseo-file-link-{}", std::process::id()));
    let matter = root.join("matter");
    write::create_dir_all(&matter).unwrap();
    write::create_dir_all(&root.join("matter2")).unwrap();
    write::write_file(&matter.join("brief.txt"), b"x").unwrap();
    write::write_file(&root.join("matter2").join("other.txt"), b"y").unwrap();
    assert!(inside(&matter.join("brief.txt"), &matter));
    assert!(!inside(
        &matter.join("..").join("matter2").join("other.txt"),
        &matter
    ));
    assert!(!inside(&root.join("matter2").join("other.txt"), &matter));
    assert!(!inside(&matter.join("gone.txt"), &matter));
    write::remove_dir_all(&root).unwrap();
}

/// A file of 260 characters or more, which `dunce` leaves in its verbatim form, lies in the
/// matter folder in its plain form; a share's verbatim form does not.
#[test]
fn a_long_verbatim_path_is_inside_its_plain_folder() {
    let mut folder = String::from(r"C:\Users\a\Documents\Matters\");
    while folder.len() < 280 {
        folder.push_str(r"A long folder name\");
    }
    let folder = folder.trim_end_matches('\\').to_owned();
    let verbatim = format!(r"\\?\{folder}");
    assert!(within(Path::new(&verbatim), Path::new(&folder)));
    assert!(within(Path::new(&folder), Path::new(&verbatim)));
    assert!(within(Path::new(&folder), Path::new(&folder)));
    let share = format!(r"\\?\UNC\server\c$\{}", folder.get(3..).unwrap_or_default());
    assert!(!within(Path::new(&share), Path::new(&folder)));
}
