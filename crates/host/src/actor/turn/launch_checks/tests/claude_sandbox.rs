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

/// A matter registered through a link (`Linked` → `real/m`): Claude Code's sandbox resolves no
/// link in a deny, so the launch prep gives both spellings, where it leads first, and the
/// sandbox denies reads of both.
#[cfg(unix)]
#[test]
fn a_matter_registered_through_a_link_is_denied_by_both_spellings() {
    use crate::agent::loads::sandbox::{ClaudeSandbox, SandboxOs, SandboxPaths};
    use crate::pipeline::tests::support::TempDir;
    use crate::write;

    let dir = TempDir::new("sandbox-matter-link");
    let root = dunce::canonicalize(dir.path()).unwrap();
    write::create_dir_all(&root.join("real/m")).unwrap();
    write::symlink(&root.join("real/m"), &root.join("Linked")).unwrap();
    let (real, linked) = (root.join("real/m"), root.join("Linked"));
    let spellings = super::super::folders::matter_spellings(&[linked.clone(), real.clone()]);
    assert_eq!(spellings, [real.clone(), linked.clone()]);
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
        skill_packs: &run.skill_packs,
        notes: &run.notes,
        skills: &[],
        path: &[],
        env_file: None,
        ca_file: None,
        claude_home: &data.join("agents/claude-acp/home"),
        data: &run.data_deny.spellings,
        other_matters: &spellings,
    };
    let sandbox = ClaudeSandbox::for_os(SandboxOs::Mac, &paths).unwrap();
    for path in [&real, &linked] {
        let path = path.to_str().unwrap().to_owned();
        assert!(sandbox.deny_read.contains(&path), "{path}");
    }
}
