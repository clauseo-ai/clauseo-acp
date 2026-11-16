//! Claude's sandbox against the real run folder's paths. Split from `tests.rs` for its size.

use std::path::{Path, PathBuf};

use super::folders;

/// Claude's sandbox opens only the named folders of the real run folder: never the run folder
/// whole, the relay's token, agent-browser's folder, its config or the browser's profile (the
/// visited sites' cookies), but the browser's downloads.
#[test]
#[cfg_attr(windows, ignore = "Claude's sandbox is macOS and Linux only")]
fn claudes_sandbox_opens_no_secret_of_the_real_run_folder() {
    use crate::agent::loads::sandbox::{ClaudeSandbox, SandboxOs, SandboxPaths};
    let data = PathBuf::from("/Users/l/Library/Application Support/Clauseo");
    let run = folders(&data, 1);
    let tmp = crate::agent::temp::temp_dir(&run.root);
    let short = PathBuf::from("/tmp/clsb-abcd1234");
    let matter = PathBuf::from("/Users/l/Clients/A");
    let paths = SandboxPaths {
        matter: &matter,
        work: &run.work,
        tmp: &tmp,
        claude_tmp: Some(&short),
        downloads: &run.downloads,
        browser: &run.browser,
        skill_packs: &run.skill_packs,
        notes: &run.notes,
        skills: &[],
        path: &[],
        env_file: Some(&run.root.join(crate::agent::shell_path::ENV_FILE)),
        ca_file: Some(&data.join("tls/roots.pem")),
        claude_home: &data.join("agents/claude-acp/home"),
        data: &run.data_deny.spellings,
        other_matters: &[],
    };
    let sandbox = ClaudeSandbox::for_os(SandboxOs::Mac, &paths).unwrap();
    let text = |path: &Path| path.to_str().unwrap().to_owned();
    for secret in [
        &run.root,
        &run.relay_token,
        &run.agent_browser,
        &run.browser_config,
        &run.browser_profile,
        &run.browser,
    ] {
        let secret = text(secret);
        for open in sandbox.allow_read.iter().chain(&sandbox.allow_write) {
            assert!(!secret.starts_with(open.as_str()), "{open} opens {secret}");
        }
    }
    assert!(sandbox.allow_read.contains(&text(&run.downloads)));
    assert!(text(&run.downloads).ends_with("agent-browser/downloads"));
    assert!(sandbox.deny_read.contains(&text(&data)));
}

/// A data folder reached through a link: the launch's deny spellings start with its real path,
/// and the sandbox denies the session's guard folder (Chrome's profile in it), `<data>/browser`
/// and `<data>/browsers` by that real path as well as by the link, for reads and writes.
#[test]
#[cfg(unix)]
fn the_browser_profile_under_a_linked_data_folder_is_denied_by_its_real_path() {
    use crate::agent::loads::sandbox::{ClaudeSandbox, SandboxOs, SandboxPaths};
    use crate::write;
    let root = super::scratch("linked-browser");
    let real = root.join("real");
    let link = root.join("link");
    write::create_dir_all(&real).unwrap();
    write::symlink(&real, &link).unwrap();
    let real = dunce::canonicalize(&real).unwrap();
    let run = folders(&link, 1);
    let matter = root.join("matter");
    let deny = super::super::folders::launch_deny(&run, &matter, &link, None);
    assert_eq!(deny.spellings, [real.clone(), link.clone()]);
    let short = PathBuf::from("/tmp/clsb-abcd1234");
    let tmp = crate::agent::temp::temp_dir(&run.root);
    let paths = SandboxPaths {
        matter: &matter,
        work: &run.work,
        tmp: &tmp,
        claude_tmp: Some(&short),
        downloads: &run.downloads,
        browser: &run.browser,
        skill_packs: &run.skill_packs,
        notes: &run.notes,
        skills: &[],
        path: &[],
        env_file: None,
        ca_file: None,
        claude_home: &link.join("agents/claude-acp/home"),
        data: &deny.spellings,
        other_matters: &[],
    };
    let guard = run.browser.strip_prefix(&link).unwrap();
    assert!(guard.starts_with("browser/run"), "{}", guard.display());
    let profile = real.join(guard).join("chrome-profile-1-0000abcd");
    for os in [SandboxOs::Mac, SandboxOs::Linux] {
        let sandbox = ClaudeSandbox::for_os(os, &paths).unwrap();
        for denied in [
            real.join(guard),
            real.join("browser"),
            real.join("browsers"),
        ] {
            let denied = denied.to_str().unwrap().to_owned();
            assert!(sandbox.deny_read.contains(&denied), "{os:?} read {denied}");
            assert!(
                sandbox.deny_write.contains(&denied),
                "{os:?} write {denied}"
            );
        }
        for open in sandbox.allow_read.iter().chain(&sandbox.allow_write) {
            assert!(
                !profile.starts_with(open),
                "{os:?}: {open} opens the profile"
            );
        }
    }
    write::remove_dir_all(&root).unwrap();
}
