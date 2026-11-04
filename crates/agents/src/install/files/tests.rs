use super::*;

#[test]
fn only_share_paths_are_unc() {
    assert!(is_unc(r"\\server\share\bundle\npm"));
    assert!(is_unc(r"\\?\UNC\server\share\bundle\npm"));
    assert!(is_unc(r"\\?\unc\server\share\npm"));
    assert!(is_unc(r"\\.\UNC\server\share\npm"));
    assert!(is_unc("//server/share/bundle/npm"));
    assert!(!is_unc(r"C:\bundle\npm"));
    assert!(!is_unc("C:/bundle/npm"));
    assert!(!is_unc(r"\\?\C:\bundle\npm"));
    assert!(!is_unc(r"\\.\C:\bundle\npm"));
    assert!(!is_unc("/home/u/bundle/npm"));
    assert!(!is_unc(r"\bundle\npm"));
}
