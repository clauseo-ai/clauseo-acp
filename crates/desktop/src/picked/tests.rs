use std::path::{MAIN_SEPARATOR, Path};

use super::display;

#[test]
fn a_folder_under_home_is_home_relative_with_either_separator() {
    let home = Path::new("/Users/rohan");
    let acme = Path::new("/Users/rohan/Documents/Acme");
    assert_eq!(display(acme, Some(home), '/'), "~/Documents/Acme");
    assert_eq!(display(acme, Some(home), '\\'), "~\\Documents\\Acme");
}

#[test]
fn home_itself_is_a_tilde() {
    let home = Path::new("/Users/rohan");
    assert_eq!(display(home, Some(home), '/'), "~");
    assert_eq!(display(Path::new("/Users/rohan/"), Some(home), '/'), "~");
}

#[test]
fn a_folder_outside_home_is_shown_in_full() {
    let home = Path::new("/Users/rohan");
    assert_eq!(
        display(Path::new("/Volumes/Work/Acme"), Some(home), '/'),
        "/Volumes/Work/Acme"
    );
    // A sibling whose name starts with the home's name is not under home.
    assert_eq!(
        display(Path::new("/Users/rohan2/Acme"), Some(home), '/'),
        "/Users/rohan2/Acme"
    );
}

#[test]
fn every_folder_is_shown_in_full_without_a_home() {
    assert_eq!(
        display(Path::new("/Users/rohan/Documents/Acme"), None, '/'),
        "/Users/rohan/Documents/Acme"
    );
}

#[test]
fn a_root_is_shown_as_it_is_even_when_it_is_home() {
    assert_eq!(display(Path::new("/"), Some(Path::new("/")), '/'), "/");
    assert_eq!(
        display(Path::new("/"), Some(Path::new("/Users/rohan")), '/'),
        "/"
    );
}

#[cfg(windows)]
#[test]
fn a_drive_root_is_shown_as_it_is() {
    let home = Path::new("C:\\Users\\rohan");
    assert_eq!(display(Path::new("C:\\"), Some(home), '\\'), "C:\\");
    assert_eq!(display(Path::new("D:\\"), Some(home), '\\'), "D:\\");
}

#[cfg(windows)]
#[test]
fn a_folder_under_home_on_windows_uses_the_os_separator() {
    let home = Path::new("C:\\Users\\rohan");
    let acme = Path::new("C:\\Users\\rohan\\Documents\\Acme");
    assert_eq!(
        display(acme, Some(home), MAIN_SEPARATOR),
        "~\\Documents\\Acme"
    );
}

#[cfg(not(windows))]
#[test]
fn a_folder_under_home_on_unix_uses_the_os_separator() {
    let home = Path::new("/Users/rohan");
    let acme = Path::new("/Users/rohan/Documents/Acme");
    assert_eq!(
        display(acme, Some(home), MAIN_SEPARATOR),
        "~/Documents/Acme"
    );
}
