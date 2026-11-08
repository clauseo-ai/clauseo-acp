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

/// A link at `place` (`agents` or `agents/npm-cache`) to a folder outside the data folder:
/// emptying the npm cache (each try's start, the worker's start, after `Ready`) skips it,
/// removing nothing out there and leaving the link itself.
#[cfg(unix)]
fn a_link_on_the_way_to_the_npm_cache(place: &str) {
    let data = crate::write::TestDir::new("npm-cache-linked");
    let outside = crate::write::TestDir::new("npm-cache-linked-canary");
    let canary = outside.file("brief.docx", b"privileged");
    // What the cache would hold were the link at `agents`.
    let held = outside.file("npm-cache/_cacache/content-v2/entry", b"held");
    let link = place
        .split('/')
        .fold(data.0.clone(), |path, part| path.join(part));
    write::create_dir_all(link.parent().unwrap()).unwrap();
    write::symlink(&outside.0, &link).unwrap();
    empty_npm_cache(&data.0);
    for path in [&canary, &held] {
        assert!(path.is_file(), "{} was removed", path.display());
    }
    let meta = std::fs::symlink_metadata(&link).expect("the link is left as it is");
    assert!(meta.file_type().is_symlink());
}

#[cfg(unix)]
#[test]
fn emptying_the_npm_cache_skips_a_link_at_agents() {
    a_link_on_the_way_to_the_npm_cache("agents");
}

#[cfg(unix)]
#[test]
fn emptying_the_npm_cache_skips_a_link_at_npm_cache() {
    a_link_on_the_way_to_the_npm_cache("agents/npm-cache");
}

/// A link inside a real npm cache, to a folder outside: the cache is emptied, the link goes
/// as a link, and nothing it points at is touched.
#[cfg(unix)]
#[test]
fn emptying_the_npm_cache_removes_a_link_inside_it_not_its_target() {
    let data = crate::write::TestDir::new("npm-cache-inner-link");
    let outside = crate::write::TestDir::new("npm-cache-inner-link-canary");
    let canary = outside.file("exhibits/a.pdf", b"exhibit");
    let entry = data.file("agents/npm-cache/_cacache/index-v5/entry", b"x");
    let link = npm_cache(&data.0).join("_cacache").join("linked");
    write::symlink(&outside.0, &link).unwrap();
    empty_npm_cache(&data.0);
    assert!(canary.is_file(), "the link's target was emptied");
    assert!(!entry.exists(), "the cache was not emptied");
    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "the link is left"
    );
}
