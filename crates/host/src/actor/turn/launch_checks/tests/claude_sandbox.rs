//! Claude's sandbox against the real run folder's paths. Split from `tests.rs` for its size.

use std::path::{Path, PathBuf};

use super::folders;
use crate::agent::loads::sandbox::OtherMatter;

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
        matter_walked: None,
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

/// The sandbox of a session in `matter` (as stored, and where it leads) beside `others`, the
/// run folders under a Mac-like `<data>`, on macOS.
#[cfg(unix)]
fn sandbox_beside(
    matter: &Path,
    others: &[OtherMatter],
) -> Result<Option<crate::agent::loads::sandbox::ClaudeSandbox>, crate::error::Error> {
    use crate::agent::loads::sandbox::{ClaudeSandbox, SandboxOs, SandboxPaths};
    let data = PathBuf::from("/Users/l/Library/Application Support/Clauseo");
    let run = folders(&data, 1);
    let tmp = crate::agent::temp::temp_dir(&run.root);
    let short = PathBuf::from("/tmp/clsb-abcd1234");
    let walked = super::super::folders::walked(matter);
    let paths = SandboxPaths {
        matter,
        matter_walked: walked.as_deref(),
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
        other_matters: others,
    };
    ClaudeSandbox::for_launch(SandboxOs::Mac, &paths)
}

/// `stored` registered as the matter `Other`, walked as the launch prep walks it.
#[cfg(unix)]
fn other_at(stored: &Path) -> Vec<OtherMatter> {
    let mut others = vec![OtherMatter::at(stored)];
    others[0].name = "Other".to_owned();
    super::super::folders::matter_spellings(&mut others);
    others
}

/// A matter registered through a link (`Linked` → `real/m`): Claude Code's sandbox resolves no
/// link in a deny, so the launch prep gives where it leads beside it as stored, and the sandbox
/// denies reads of both, with no notice.
#[cfg(unix)]
#[test]
fn a_matter_registered_through_a_link_is_denied_by_both_spellings() {
    use crate::pipeline::tests::support::TempDir;
    use crate::write;

    let dir = TempDir::new("sandbox-matter-link");
    let root = dunce::canonicalize(dir.path()).unwrap();
    write::create_dir_all(&root.join("real/m")).unwrap();
    write::symlink(&root.join("real/m"), &root.join("Linked")).unwrap();
    let (real, linked) = (root.join("real/m"), root.join("Linked"));
    let others = other_at(&linked);
    assert_eq!(others[0].walked.as_deref(), Some(real.as_path()));
    let mut plain = vec![OtherMatter::at(real.clone())];
    super::super::folders::matter_spellings(&mut plain);
    assert_eq!(plain[0].walked, None, "a real folder has one spelling");
    let matter = PathBuf::from("/Users/l/Clients/A");
    let sandbox = sandbox_beside(&matter, &others).unwrap().unwrap();
    for path in [&real, &linked] {
        let path = path.to_str().unwrap().to_owned();
        assert!(sandbox.deny_read.contains(&path), "{path}");
    }
    assert!(sandbox.links_left_out.is_empty());
}

/// Another matter's folder swapped for a real link to `/`, to the session's own matter's parent
/// or to `/usr` (the security review of #1153, M1): where it leads is never denied, which would
/// break every command; the link itself still is, the launch goes ahead, and the matter is
/// named for the launch's notice.
#[cfg(unix)]
#[test]
fn a_link_to_a_place_the_shell_needs_is_left_out_and_told() {
    use crate::pipeline::tests::support::TempDir;
    use crate::write;

    let dir = TempDir::new("sandbox-matter-link-open");
    let root = dunce::canonicalize(dir.path()).unwrap();
    let parent = root.join("clients");
    let matter = parent.join("Own");
    write::create_dir_all(&matter).unwrap();
    for (name, target) in [
        ("root", PathBuf::from("/")),
        ("parent", parent.clone()),
        ("usr", PathBuf::from("/usr")),
    ] {
        let link = root.join(format!("link-{name}"));
        write::symlink(&target, &link).unwrap();
        let others = other_at(&link);
        let walked = others[0].walked.clone().expect("the link leads elsewhere");
        assert_eq!(walked, dunce::canonicalize(&target).unwrap(), "{name}");
        let sandbox = sandbox_beside(&matter, &others)
            .expect("no refusal")
            .expect("a sandbox");
        let walked = walked.to_str().unwrap().to_owned();
        assert!(
            !sandbox.deny_read.contains(&walked),
            "{name}: {walked} denied"
        );
        assert!(!sandbox.deny_read.iter().any(|deny| deny == "/"), "{name}");
        let link = link.to_str().unwrap().to_owned();
        assert!(
            sandbox.deny_read.contains(&link),
            "{name}: the link not denied"
        );
        assert_eq!(sandbox.links_left_out, ["Other"], "{name}");
    }
}

/// Stage C's Mac linked-data row on 0fcfd522b: `sweep-data` a real link to `sweep-data.real`,
/// the host configured with the link. The launch prep re-spells the run under the real path
/// (`folders::respell_run`), so every path in the shells' `PATH` file and every allow entry of
/// Claude's sandbox is the real spelling (Seatbelt can't walk the denied link), while both
/// spellings stay denied.
#[cfg(unix)]
#[test]
fn under_a_linked_data_folder_claude_is_handed_only_real_spellings() {
    use clauseo_protocol::AgentKind;

    use crate::agent::loads::sandbox::{ClaudeSandbox, SandboxOs, SandboxPaths};
    use crate::pipeline::tests::support::TempDir;
    use crate::write;

    let dir = TempDir::new("sandbox-linked-data");
    let root = dunce::canonicalize(dir.path()).unwrap();
    let (real, link) = (root.join("sweep-data.real"), root.join("sweep-data"));
    write::create_dir_all(&real).unwrap();
    write::symlink(&real, &link).unwrap();
    let mut run = folders(&link, 1);
    run.claude_tmp = Some(PathBuf::from("/tmp/clsb-abcd1234"));
    let made = super::super::folders::respell_run(&mut run, &link);
    assert_eq!(made, real);
    write::create_dir_all(&run.root).unwrap();
    let runtime = "agents/runtime/clauseo-python-3.13.9-aarch64/bin";
    write::create_dir_all(&real.join(runtime)).unwrap();
    let dirs = vec![run.real(&link.join(runtime))];
    let home = run.real(&link.join("agents/claude-acp/home"));
    let stored = root.join("Clients/A");
    write::create_dir_all(&stored).unwrap();
    super::super::folders::fences(&mut run, &stored, &made, Some(&home));
    let ca = run.real(&link.join("tls/roots.pem"));
    let path = crate::agent::shell_path::prepare(
        dirs,
        true,
        Some(AgentKind::Claude),
        &run.root,
        Some(&ca),
    );
    let link_text = format!("{}/", link.to_str().unwrap());
    let env_file = path.env_file.clone().expect("the PATH file");
    assert!(env_file.starts_with(&real));
    let env = std::fs::read_to_string(&env_file).unwrap();
    assert!(env.contains(real.join(runtime).to_str().unwrap()), "{env}");
    assert!(!env.contains(&link_text), "{env}");
    let tmp = crate::agent::temp::temp_dir(&run.root);
    let paths = SandboxPaths {
        matter: &stored,
        matter_walked: run.matter_walked.as_deref(),
        work: &run.work,
        tmp: &tmp,
        claude_tmp: run.claude_tmp.as_deref(),
        downloads: &run.downloads,
        skill_packs: &run.skill_packs,
        notes: &run.notes,
        skills: &[],
        path: &path.dirs,
        env_file: path.env_file.as_deref(),
        ca_file: path.ca_file.as_deref(),
        claude_home: &home,
        data: &run.data_deny.spellings,
        other_matters: &[],
    };
    let sandbox = ClaudeSandbox::for_os(SandboxOs::Mac, &paths).unwrap();
    let lists = [&sandbox.allow_write, &sandbox.allow_read, &sandbox.quiet];
    for open in lists.into_iter().flatten() {
        assert!(!open.starts_with(&link_text), "{open} is link-spelled");
    }
    assert!(
        sandbox
            .allow_write
            .contains(&run.work.to_str().unwrap().to_owned())
    );
    assert!(run.work.starts_with(&real) && run.notes.starts_with(&real));
    for spelling in [&real, &link] {
        let spelling = spelling.to_str().unwrap().to_owned();
        assert!(
            sandbox.deny_read.contains(&spelling),
            "{spelling} not denied"
        );
    }
}
