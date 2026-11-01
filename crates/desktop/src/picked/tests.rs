use std::path::{MAIN_SEPARATOR, Path};

use super::display;

#[test]
fn a_folder_under_home_is_the_parts_below_home_with_either_separator() {
    let home = Path::new("/Users/rmehra");
    let acme = Path::new("/Users/rmehra/Documents/Acme");
    assert_eq!(display(acme, Some(home), '/'), "Documents/Acme");
    assert_eq!(display(acme, Some(home), '\\'), "Documents\\Acme");
}

#[test]
fn a_folder_directly_under_home_is_its_name_alone() {
    let home = Path::new("/Users/rmehra");
    assert_eq!(
        display(Path::new("/Users/rmehra/work"), Some(home), '/'),
        "work"
    );
}

#[test]
fn home_itself_is_shown_in_full() {
    let home = Path::new("/Users/rmehra");
    assert_eq!(display(home, Some(home), '/'), "/Users/rmehra");
}

#[test]
fn a_folder_outside_home_is_shown_in_full() {
    let home = Path::new("/Users/rmehra");
    assert_eq!(
        display(Path::new("/Volumes/Work/Acme"), Some(home), '/'),
        "/Volumes/Work/Acme"
    );
    // A sibling whose name starts with the home's name is not under home.
    assert_eq!(
        display(Path::new("/Users/rmehra2/Acme"), Some(home), '/'),
        "/Users/rmehra2/Acme"
    );
}

#[test]
fn every_folder_is_shown_in_full_without_a_home() {
    assert_eq!(
        display(Path::new("/Users/rmehra/Documents/Acme"), None, '/'),
        "/Users/rmehra/Documents/Acme"
    );
}

#[test]
fn a_root_as_home_counts_as_no_home() {
    assert_eq!(
        display(
            Path::new("/Users/rmehra/Documents/Acme"),
            Some(Path::new("/")),
            '/'
        ),
        "/Users/rmehra/Documents/Acme"
    );
}

#[test]
fn a_root_is_shown_as_it_is_even_when_it_is_home() {
    assert_eq!(display(Path::new("/"), Some(Path::new("/")), '/'), "/");
    assert_eq!(
        display(Path::new("/"), Some(Path::new("/Users/rmehra")), '/'),
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
fn a_drive_root_as_home_counts_as_no_home() {
    assert_eq!(
        display(
            Path::new("C:\\Users\\rohan\\Documents\\Acme"),
            Some(Path::new("C:\\")),
            '\\'
        ),
        "C:\\Users\\rohan\\Documents\\Acme"
    );
}

#[cfg(windows)]
#[test]
fn a_folder_under_home_on_windows_uses_the_os_separator() {
    let home = Path::new("C:\\Users\\rohan");
    let acme = Path::new("C:\\Users\\rohan\\Documents\\Acme");
    assert_eq!(display(acme, Some(home), MAIN_SEPARATOR), "Documents\\Acme");
}

#[cfg(not(windows))]
#[test]
fn a_folder_under_home_on_unix_uses_the_os_separator() {
    let home = Path::new("/Users/rmehra");
    let acme = Path::new("/Users/rmehra/Documents/Acme");
    assert_eq!(display(acme, Some(home), MAIN_SEPARATOR), "Documents/Acme");
}
