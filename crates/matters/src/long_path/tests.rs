//! [`verbatim`] on every OS, and [`long_path`] where it is the identity.

use super::verbatim;

/// `text` in UTF-16 units.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// The verbatim form of `text` as a string, or `None`.
fn of(text: &str) -> Option<String> {
    verbatim(&wide(text)).map(|units| String::from_utf16(&units).unwrap())
}

#[test]
fn a_drive_path_gets_the_verbatim_prefix() {
    assert_eq!(of(r"C:\x").as_deref(), Some(r"\\?\C:\x"));
    assert_eq!(
        of(r"d:\Users\a\history.git\objects\ab").as_deref(),
        Some(r"\\?\d:\Users\a\history.git\objects\ab")
    );
    assert_eq!(of(r"C:\").as_deref(), Some(r"\\?\C:\"));
}

#[test]
fn a_unc_path_becomes_verbatim_unc() {
    assert_eq!(
        of(r"\\srv\share\x").as_deref(),
        Some(r"\\?\UNC\srv\share\x")
    );
    assert_eq!(of(r"\\srv\share").as_deref(), Some(r"\\?\UNC\srv\share"));
}

#[test]
fn a_verbatim_or_device_path_is_left_as_it_is() {
    for text in [
        r"\\?\C:\x",
        r"\\?\UNC\srv\share\x",
        r"\\?\Volume{0b1c}\x",
        r"\\.\C:\x",
        r"\\.\pipe\name",
    ] {
        assert_eq!(of(text), None, "{text}");
    }
}

#[test]
fn a_path_that_is_not_absolute_is_left_as_it_is() {
    for text in [
        "", "x", r"x\y", r"\x", r"C:x", "C:", r"\\", r"\\\x", r"1:\x",
    ] {
        assert_eq!(of(text), None, "{text:?}");
    }
}

/// A Devanagari name is carried unit for unit: its length counts in UTF-16 units, as the 260
/// limit does.
#[test]
fn a_devanagari_path_is_carried_unit_for_unit() {
    let path = r"C:\Users\अनुबंध\मामले\history.git";
    let long = verbatim(&wide(path)).unwrap();
    assert_eq!(String::from_utf16(&long).unwrap(), format!(r"\\?\{path}"));
    assert_eq!(long.len(), wide(path).len() + 4);
    let unc = verbatim(&wide(r"\\सर्वर\share\अनुबंध")).unwrap();
    assert_eq!(
        String::from_utf16(&unc).unwrap(),
        r"\\?\UNC\सर्वर\share\अनुबंध"
    );
}

/// A path whose UTF-16 is not well formed (a lone surrogate, which a Windows name may hold)
/// keeps its units.
#[test]
fn ill_formed_utf16_is_kept() {
    let mut path = wide(r"C:\a");
    path.push(0xD800);
    let long = verbatim(&path).unwrap();
    assert_eq!(long.get(4..), Some(path.as_slice()));
}

#[cfg(not(windows))]
#[test]
fn long_path_is_the_identity_off_windows() {
    use std::path::Path;
    for text in ["/a/b", "relative/x", "/a/../b", r"C:\x"] {
        assert_eq!(super::long_path(Path::new(text)), Path::new(text));
    }
}
