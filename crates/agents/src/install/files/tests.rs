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

#[test]
fn only_a_windows_data_folder_on_a_share_is_refused() {
    assert!(data_dir_on_share(r"\\srv\share\Clauseo", Os::Win32));
    assert!(data_dir_on_share(r"\\?\UNC\srv\share\Clauseo", Os::Win32));
    assert!(!data_dir_on_share(
        r"C:\Users\u\AppData\Local\Clauseo",
        Os::Win32
    ));
    assert!(!data_dir_on_share(r"\\?\C:\Users\u\Clauseo", Os::Win32));
    // A backslash is a file name's character on Unix, so nothing there is a share.
    assert!(!data_dir_on_share(r"\\srv\share\Clauseo", Os::Linux));
    assert!(!data_dir_on_share(
        "/home/u/.local/share/clauseo",
        Os::Linux
    ));
    assert!(!data_dir_on_share("//srv/share/Clauseo", Os::Darwin));
}
