//! The `agent-browser` command's copy in `<data>/tools` (`clauseo_profile::tools`), on a real
//! disk: made once, made again only when the SHA-256 differs, a program (0755 on Unix), never a
//! link, with no temp file left; a missing sidecar writes nothing.

use clauseo_profile::tools::install_agent_browser;

use super::{Scratch, profile_paths, read, write};

#[test]
fn the_copy_is_made_once_and_again_only_when_the_hash_differs() {
    let root = Scratch::new("tools-copy");
    let paths = profile_paths(&root);
    let source = root.join("app").join("clauseo-agent-browser");
    write::write(&source, b"v1").unwrap();
    let (target, wrote) = install_agent_browser(&source, &paths).unwrap();
    assert!(wrote);
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
    assert!(!again, "an unchanged copy is not written again");
    write::write(&source, b"v2").unwrap();
    assert!(install_agent_browser(&source, &paths).unwrap().1);
    assert_eq!(read(&target), b"v2");
    // A copy changed in place is put back to the bundled bytes.
    write::write(&target, b"tampered").unwrap();
    assert!(install_agent_browser(&source, &paths).unwrap().1);
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
