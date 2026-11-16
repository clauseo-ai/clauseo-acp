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

fn agent() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

/// A link at `place` (`agents`, `agents/npm-cache` or the agent's folder in it) to a folder
/// outside the data folder: emptying the agent's npm cache (each try's start, the worker's
/// start, after `Ready`) skips it, removing nothing out there and leaving the link itself.
#[cfg(unix)]
fn a_link_on_the_way_to_the_npm_cache(place: &str) {
    let data = crate::write::TestDir::new("npm-cache-linked");
    let outside = crate::write::TestDir::new("npm-cache-linked-canary");
    let canary = outside.file("brief.docx", b"privileged");
    // What the cache would hold were the link at `agents`.
    let held = outside.file("npm-cache/claude-acp/_cacache/content-v2/entry", b"held");
    let link = place
        .split('/')
        .fold(data.0.clone(), |path, part| path.join(part));
    write::create_dir_all(link.parent().unwrap()).unwrap();
    write::symlink(&outside.0, &link).unwrap();
    empty_npm_cache(&data.0, &agent());
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

#[cfg(unix)]
#[test]
fn emptying_the_npm_cache_skips_a_link_at_the_agent_s_cache() {
    a_link_on_the_way_to_the_npm_cache("agents/npm-cache/claude-acp");
}

/// A link inside a real npm cache, to a folder outside: the cache is emptied, the link goes
/// as a link, and nothing it points at is touched.
#[cfg(unix)]
#[test]
fn emptying_the_npm_cache_removes_a_link_inside_it_not_its_target() {
    let data = crate::write::TestDir::new("npm-cache-inner-link");
    let outside = crate::write::TestDir::new("npm-cache-inner-link-canary");
    let canary = outside.file("exhibits/a.pdf", b"exhibit");
    let entry = data.file("agents/npm-cache/claude-acp/_cacache/index-v5/entry", b"x");
    let link = npm_cache(&data.0, &agent()).join("_cacache").join("linked");
    write::symlink(&outside.0, &link).unwrap();
    empty_npm_cache(&data.0, &agent());
    assert!(canary.is_file(), "the link's target was emptied");
    assert!(!entry.exists(), "the cache was not emptied");
    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "the link is left"
    );
}

/// Emptying one agent's npm cache leaves the other agent's, which its install may be using.
#[test]
fn emptying_one_agent_s_npm_cache_leaves_the_other_s() {
    let data = crate::write::TestDir::new("npm-cache-per-agent");
    let own = data.file("agents/npm-cache/claude-acp/_cacache/index-v5/entry", b"x");
    let other = data.file("agents/npm-cache/codex-acp/_cacache/index-v5/entry", b"y");
    empty_npm_cache(&data.0, &agent());
    assert!(!own.exists(), "the agent's own cache was not emptied");
    assert!(other.is_file(), "the other agent's cache was emptied");
}

/// What an older Clauseo left in the shared places goes at the start: the one npm cache's
/// entries and the digest-named package tarballs (and their `.part` files). Each agent's own
/// cache and package folder, Node's and Python's archives and anything else stay.
#[test]
fn the_start_removes_what_an_older_clauseo_left_in_the_shared_places() {
    let data = crate::write::TestDir::new("legacy-shared");
    let known = [agent(), AgentId::new("codex-acp").unwrap()];
    let legacy_cache = data.file("agents/npm-cache/_cacache/index-v5/entry", b"x");
    let legacy_logs = data.file("agents/npm-cache/_logs/one.log", b"x");
    let own_cache = data.file("agents/npm-cache/codex-acp/_cacache/index-v5/entry", b"x");
    let tarball = data.file(&format!("agents/downloads/{}.tgz", "ab".repeat(64)), b"x");
    let part = data.file(
        &format!("agents/downloads/{}.tgz.part", "cd".repeat(64)),
        b"x",
    );
    let node = data.file("agents/downloads/node-v24.22.0-darwin-arm64.tar.gz", b"x");
    let python = data.file("agents/downloads/clauseo-python-3.13.18.tar.gz", b"x");
    let own_tarball = data.file(
        &format!("agents/downloads/claude-acp/{}.tgz", "ef".repeat(64)),
        b"x",
    );
    let named = data.file("agents/downloads/notes.tgz", b"x");
    remove_legacy_shared(&data.0, &known);
    for gone in [&legacy_cache, &legacy_logs, &tarball, &part] {
        assert!(!gone.exists(), "{} is left", gone.display());
    }
    for kept in [&own_cache, &node, &python, &own_tarball, &named] {
        assert!(kept.is_file(), "{} was removed", kept.display());
    }
}
