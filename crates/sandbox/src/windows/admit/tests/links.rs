//! The gate on links: the agents' folder, the credential link and the Claude store's files.

use super::*;

/// `<root>/data2/agents` (a data folder of its own; the world's has a real `agents`) as a link to
/// `target` (a symbolic link: a name-surrogate reparse point, as a
/// junction is); `None` where this process may not make one.
fn link_agents(w: &World, target: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    let link = Some(w.dir.symlink("data2/agents", target));
    #[cfg(windows)]
    let link = w.dir.symlink_dir("data2/agents", target).ok();
    link
}

#[test]
fn agents_as_a_link_refuses_the_launch() {
    // Moved aside, then linked to a folder elsewhere: what the canonical paths compare against
    // could be swapped under them.
    let w = World::new("admit-agents-link");
    w.dir.dir("elsewhere/agents/claude-acp/node");
    w.dir.dir("data2");
    let Some(_link) = link_agents(&w, &w.at("elsewhere/agents")) else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI may make links, and this run can't"
        );
        eprintln!("skipped: no right to make a link");
        return;
    };
    let policy = w.builder().build().unwrap();
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let refused = admit_with(&policy, &w.at("data2"), &[], check, &FakeGroups::default());
    let Err(Error::FolderUnsupported { support, .. }) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(support, FolderRefusal::ReparsePoint);
}

#[cfg(unix)]
#[test]
fn a_symlinked_credential_link_or_store_file_refuses_and_a_hard_link_passes() {
    let w = World::new("admit-unresolved-links");
    let secret = w.dir.file("elsewhere/secret.json", "{}");
    let auth = format!("{STATE}/auth.json");
    let check = |_: &Path| Ok(FolderSupport::Ok);
    let groups = FakeGroups::default();
    // A hard link, as the host makes the credential link: fine.
    let real = w.dir.file("elsewhere/auth.json", "{}");
    w.dir.hard_link(&auth, &real);
    let linked = w.builder().credential_link(w.at(&auth)).build().unwrap();
    let (mut ledger, before) = ledger_with_an_entry(&w);
    let admitted = admit_with(&linked, &w.at("data"), &[], check, &groups).unwrap();
    assert_ne!(
        plan(&admitted, &sids(), matter_one())
            .unwrap()
            .grants()
            .len(),
        0
    );
    // A symbolic link there instead: refused, nothing planned or written.
    crate::test_support::remove(&w.at(&auth));
    w.dir.symlink(&auth, &secret);
    let policy = w.builder().credential_link(w.at(&auth)).build().unwrap();
    let refused = prepare(&policy, &w.at("data"), &[], check, &groups, &mut ledger);
    let Err(Error::FolderUnsupported { path_kind, support }) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(
        (path_kind, support),
        (PathKind::CredentialLink, FolderRefusal::ReparsePoint)
    );
    assert_eq!(std::fs::read(ledger.path()).unwrap(), before);
    // And a Claude store file that is a symbolic link.
    let store_file = format!("{STORE}/.credentials.json");
    w.dir.symlink(&store_file, &secret);
    let policy = w.builder().claude_store(w.at(STORE)).build().unwrap();
    let refused = admit_with(&policy, &w.at("data"), &[], check, &groups);
    let Err(Error::FolderUnsupported { path_kind, support }) = refused else {
        panic!("not refused: {refused:?}");
    };
    assert_eq!(
        (path_kind, support),
        (PathKind::ClaudeStoreFile, FolderRefusal::ReparsePoint)
    );
}
