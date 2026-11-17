//! The `agent-browser` command's copy in `<data>/tools` (`clauseo_profile::tools`), on a real
//! disk: made once, made again only when the SHA-256 differs, a program (0755 on Unix), never a
//! link, with no temp file left; a missing sidecar writes nothing.

use clauseo_profile::tools::{Installed, install_agent_browser};

use super::{Scratch, profile_paths, read, write};

#[test]
fn the_copy_is_made_once_and_again_only_when_the_hash_differs() {
    let root = Scratch::new("tools-copy");
    let paths = profile_paths(&root);
    let source = root.join("app").join("clauseo-agent-browser");
    write::write(&source, b"v1").unwrap();
    let (target, installed) = install_agent_browser(&source, &paths).unwrap();
    assert_eq!(installed, Installed::Copied);
    assert!(installed.wrote());
    let name = format!("agent-browser{}", std::env::consts::EXE_SUFFIX);
    assert_eq!(target, root.join("tools").join(name));
    assert_eq!(target, paths.agent_browser_command());
    assert_eq!(read(&target), b"v1");
    assert!(
        !std::fs::symlink_metadata(&target)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
    let (_, again) = install_agent_browser(&source, &paths).unwrap();
    assert_eq!(
        again,
        Installed::UpToDate,
        "an unchanged copy is not written again"
    );
    assert!(!again.wrote());
    write::write(&source, b"v2").unwrap();
    assert!(install_agent_browser(&source, &paths).unwrap().1.wrote());
    assert_eq!(read(&target), b"v2");
    // A copy changed in place is put back to the bundled bytes.
    write::write(&target, b"tampered").unwrap();
    assert!(install_agent_browser(&source, &paths).unwrap().1.wrote());
    assert_eq!(read(&target), b"v2");
    let names: Vec<_> = std::fs::read_dir(paths.tools())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1, "no temp file left: {names:?}");
}

#[test]
fn a_missing_sidecar_is_an_error_and_writes_nothing() {
    let root = Scratch::new("tools-missing");
    let paths = profile_paths(&root);
    let source = root.join("app").join("clauseo-agent-browser");
    assert!(install_agent_browser(&source, &paths).is_err());
    assert!(!paths.agent_browser_command().exists());
}

#[test]
fn the_no_git_copy_is_put_back_at_each_start_and_gives_the_sources_hash() {
    use clauseo_profile::tools::install_no_git;
    use sha2::{Digest as _, Sha256};

    let root = Scratch::new("tools-no-git");
    let paths = profile_paths(&root);
    let source = root.join("app").join("clauseo-mcp");
    write::write(&source, b"mcp v1").unwrap();
    let want: [u8; 32] = Sha256::digest(b"mcp v1").into();
    let name = format!("git{}", std::env::consts::EXE_SUFFIX);
    let target = root.join("tools").join("no-git").join(name);
    assert_eq!(paths.no_git_command(), target);
    assert_eq!(
        install_no_git(&source, &paths).unwrap(),
        (Installed::Copied, want)
    );
    assert_eq!(read(&target), b"mcp v1");
    assert_eq!(
        install_no_git(&source, &paths).unwrap(),
        (Installed::UpToDate, want)
    );
    // Gone (a virus scanner's quarantine): the next start copies it again.
    write::remove_file(&target).unwrap();
    assert_eq!(
        install_no_git(&source, &paths).unwrap().0,
        Installed::Copied
    );
    assert_eq!(read(&target), b"mcp v1");
    write::write(&target, b"changed").unwrap();
    assert!(install_no_git(&source, &paths).unwrap().0.wrote());
    assert_eq!(read(&target), b"mcp v1");
    // A link at the name, to the same bytes, is replaced by a copy.
    #[cfg(unix)]
    {
        write::remove_file(&target).unwrap();
        write::symlink_dir(&source, &target).unwrap();
        assert!(install_no_git(&source, &paths).unwrap().0.wrote());
        let kind = std::fs::symlink_metadata(&target).unwrap().file_type();
        assert!(kind.is_file(), "a regular file, not a link");
    }
    let names: Vec<_> = std::fs::read_dir(target.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        names.len(),
        1,
        "the folder holds the program alone: {names:?}"
    );
}

#[test]
fn a_missing_mcp_binary_gives_no_no_git_copy() {
    let root = Scratch::new("tools-no-git-missing");
    let paths = profile_paths(&root);
    let source = root.join("app").join("clauseo-mcp");
    assert!(clauseo_profile::tools::install_no_git(&source, &paths).is_err());
    assert!(!paths.no_git_command().exists());
}

#[test]
fn the_no_git_copy_sweeps_what_lies_beside_it_but_its_own_leftovers() {
    use clauseo_profile::tools::install_no_git;

    let root = Scratch::new("tools-no-git-sweep");
    let paths = profile_paths(&root);
    let source = root.join("app").join("clauseo-mcp");
    write::write(&source, b"mcp").unwrap();
    install_no_git(&source, &paths).unwrap();
    let folder = paths.no_git_command().parent().unwrap().to_path_buf();
    for name in ["git.cmd", "git.com", "notes.txt", "desktop.ini"] {
        write::write(&folder.join(name), b"x").unwrap();
    }
    write::make_read_only(&folder.join("desktop.ini")).unwrap();
    write::fresh_dir(&folder.join("bin")).unwrap();
    write::write(&folder.join(".DS_Store"), b"finder").unwrap();
    assert_eq!(
        install_no_git(&source, &paths).unwrap().0,
        Installed::UpToDate
    );
    let mut names: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    let program = format!("git{}", std::env::consts::EXE_SUFFIX);
    assert_eq!(names, [".DS_Store".to_owned(), program]);
}
