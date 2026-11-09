//! The launch's jail, its pure parts: the set-aside names made relative, the notice, the
//! failure's message without a path, and B2's policy.

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, MatterId, ThreadId};
use clauseo_sandbox::SetAside;

use super::notice::{NAMED, When, notice, plain, relative_names};
use super::{UndoOnDrop, launch_policy, refused};
use crate::actor::turn::launch_checks::RunFolders;
use crate::config::SessionKey;
use crate::pipeline::tests::support::id;
use crate::write;

fn moved(from: &Path) -> SetAside {
    SetAside {
        from: from.to_owned(),
        to: PathBuf::from("/data/sandbox/set-aside/m/1").join(from.file_name().unwrap()),
    }
}

#[test]
fn set_aside_names_are_relative_to_the_matter_and_outside_ones_are_dropped() {
    let matter = std::env::temp_dir().join("clauseo-b2-m");
    let verbatim = PathBuf::from(format!(
        r"\\?\{}",
        matter.join("sub").join("AGENTS.md").display()
    ));
    let set_aside = [
        moved(&matter.join(".git")),
        moved(&verbatim),
        moved(&std::env::temp_dir().join("elsewhere").join("CLAUDE.md")),
        moved(Path::new("sub/.claude")),
        moved(&matter),
    ];
    assert_eq!(
        relative_names(&set_aside, &matter),
        [".git", "sub/AGENTS.md"]
    );
    // A verbatim matter root matches a plain name too.
    let root = PathBuf::from(format!(r"\\?\{}", matter.display()));
    assert_eq!(relative_names(&set_aside[..1], &root), [".git"]);
}

#[test]
fn the_verbatim_prefix_goes_for_drives_and_unc_shares_alike() {
    assert_eq!(plain(Path::new(r"\\?\C:\m\.git")), Path::new(r"C:\m\.git"));
    assert_eq!(
        plain(Path::new(r"\\?\UNC\srv\share\m\.git")),
        Path::new(r"\\srv\share\m\.git")
    );
    assert_eq!(plain(Path::new("/tmp/m/.git")), Path::new("/tmp/m/.git"));
    // Only the prefix's own form: a name that merely starts alike stays as it is.
    assert_eq!(plain(Path::new(r"\\srv\share")), Path::new(r"\\srv\share"));
}

#[test]
fn a_name_matches_the_matter_by_whole_names_never_through_dot_dot() {
    let base = std::env::temp_dir().join("clauseo-b2-prefix");
    let matter = base.join("ab");
    let set_aside = [
        // A lookalike: `abc` begins with `ab` but is another folder.
        moved(&base.join("abc").join(".git")),
        // Out of the matter and back in, or out of it: never named.
        moved(
            &matter
                .join("sub")
                .join("..")
                .join("..")
                .join("x")
                .join(".git"),
        ),
        moved(&matter.join("..").join("ab").join(".git")),
        moved(&matter.join("sub").join("CLAUDE.md")),
    ];
    assert_eq!(relative_names(&set_aside, &matter), ["sub/CLAUDE.md"]);
}

#[test]
fn letter_case_matches_on_windows_only() {
    let base = std::env::temp_dir().join("clauseo-b2-case");
    let set_aside = [moved(&base.join("matter").join(".git"))];
    let names = relative_names(&set_aside, &base.join("Matter"));
    if cfg!(windows) {
        assert_eq!(names, [".git"], "Windows' folders ignore letter case");
    } else {
        assert_eq!(names, [] as [String; 0], "another folder here");
    }
}

#[cfg(windows)]
#[test]
fn a_unc_share_names_relative_to_its_matter() {
    let matter = Path::new(r"\\srv\share\Matters\m");
    let set_aside = [
        moved(Path::new(r"\\?\UNC\srv\share\matters\M\sub\AGENTS.md")),
        moved(Path::new(r"\\?\UNC\srv\other\Matters\m\.git")),
    ];
    assert_eq!(relative_names(&set_aside, matter), ["sub/AGENTS.md"]);
}

#[test]
fn one_notice_names_the_first_ten_and_counts_the_rest() {
    let two = notice(
        &[".git".to_owned(), "sub/AGENTS.md".to_owned()],
        When::BeforeLaunch,
    );
    assert!(two.text.contains(".git, sub/AGENTS.md"), "{}", two.text);
    assert!(two.text.contains("2 protected items"), "{}", two.text);
    assert_eq!(two.kind, None);
    let many: Vec<String> = (0..NAMED + 2).map(|n| format!("f{n}/.git")).collect();
    let text = notice(&many, When::AfterSession).text;
    assert!(
        text.starts_with("After the agent's session ended"),
        "{text}"
    );
    assert!(
        text.contains("f9/.git") && !text.contains("f10/.git"),
        "{text}"
    );
    assert!(text.ends_with("and 2 more."), "{text}");
}

#[test]
fn a_failure_names_its_kind_and_never_its_path() {
    let error = refused(&clauseo_sandbox::Error::Policy(
        "the matter /Users/a/Matters/Smith holds the home folder".to_owned(),
    ));
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
    let message = error.to_string();
    assert!(
        message.contains("rules for this matter were refused"),
        "{message}"
    );
    assert!(!message.contains("/Users"), "{message}");
    let link = refused(&clauseo_sandbox::Error::ProtectedLink {
        at: PathBuf::from("sub/AGENTS.md"),
    });
    assert_eq!(link.code(), ErrorCode::MatterProtectedLink);
    let probe = refused(&clauseo_sandbox::Error::Probe(std::io::Error::other(
        "no probe",
    )));
    assert_eq!(probe.code(), ErrorCode::Internal);
    assert!(!link.to_string().contains("AGENTS.md"), "{link}");
}

#[test]
fn the_policy_grants_the_matter_the_session_tmp_and_the_install() {
    let root = std::env::temp_dir().join(format!("clauseo-b2-policy-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    let (data, matter, install) = (root.join("data"), root.join("m"), root.join("agent"));
    for folder in [&matter, &install] {
        write::create_dir_all(folder).unwrap();
    }
    let run = RunFolders::new(
        &data,
        SessionKey {
            matter: id::<MatterId>(1),
            thread: id::<ThreadId>(2),
            launch: 3,
        },
    );
    write::create_dir_all(&run.work).unwrap();
    let policy = launch_policy(&matter, &run, &install).unwrap();
    let real = |path: &Path| dunce::canonicalize(path).unwrap();
    assert_eq!(policy.matter(), real(&matter));
    assert_eq!(policy.state(), real(&run.work));
    // The session's `run/<session>/tmp`, required by the sandbox since R17 PR-D1.
    assert_eq!(policy.tmpdir(), real(&run.root).join("tmp"));
    assert_eq!(policy.runtime(), [real(&install)]);
    assert!(policy.read().is_empty() && policy.allowed_folders().is_empty());
    write::remove_dir_all(&root).unwrap();
}

/// Whether `policy`'s jail may let the agent write `path`, counted widely: `path` inside any of
/// its read-write grants (the matter, the state folder, the temp folder, the notes, the
/// credential link, the browser socket folder, the Claude store's folder whole) and none of its
/// read-only paths.
fn jail_may_write(policy: &clauseo_sandbox::Policy, path: &Path) -> bool {
    use clauseo_sandbox::{Access, PathClass};
    let granted = [
        Some(policy.matter()),
        Some(policy.state()),
        Some(policy.tmpdir()),
        policy.notes(),
        policy.credential_link(),
        policy.browser_socket_dir(),
        policy.claude_store().map(clauseo_sandbox::ClaudeStore::dir),
    ]
    .into_iter()
    .flatten()
    .any(|root| path.starts_with(root));
    granted && policy.classify(path, Access::Write) != PathClass::ReadOnly
}

/// R17 PR-D1 round 3: Codex's install under `<data>/agents/codex-acp`, with the bundled `rg`
/// the ripgrep guard checks (`codex_fence/rg_guard.rs`), is never writable in an agent's jail:
/// not in Claude's (whose install is its own) and not in Codex's, where it is the read-and-run
/// runtime. The Seatbelt profile (`macos/profile` snapshot `with_a_runtime_path_in_the_data_dir`)
/// and the Windows plan (`acl_plan/tests/data.rs`) render the same policy.
#[test]
fn the_codex_install_is_never_writable_in_an_agents_jail() {
    let root = std::env::temp_dir().join(format!("clauseo-d1-rg-jail-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    let (data, matter) = (root.join("data"), root.join("m"));
    let codex = data.join("agents/codex-acp");
    let rg = codex
        .join("node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/codex-path");
    let claude = data.join("agents/claude-acp/bin");
    for folder in [&matter, &rg, &claude] {
        write::create_dir_all(folder).unwrap();
    }
    write::write_file(&rg.join("rg"), b"rg").unwrap();
    let run = RunFolders::new(
        &data,
        SessionKey {
            matter: id::<MatterId>(1),
            thread: id::<ThreadId>(2),
            launch: 3,
        },
    );
    write::create_dir_all(&run.work).unwrap();
    let rg = dunce::canonicalize(rg.join("rg")).unwrap();
    for install in [&claude, &codex] {
        let policy = launch_policy(&matter, &run, install).unwrap();
        for path in [
            rg.as_path(),
            rg.parent().unwrap(),
            &dunce::canonicalize(&codex).unwrap(),
        ] {
            assert!(!jail_may_write(&policy, path), "{}", path.display());
        }
    }
    let codex_jail = launch_policy(&matter, &run, &codex).unwrap();
    let class = codex_jail.classify(&rg, clauseo_sandbox::Access::Write);
    assert_eq!(class, clauseo_sandbox::PathClass::ReadOnly);
    write::remove_dir_all(&root).unwrap();
}

/// A guard whose undo counts into the returned cell.
fn counted() -> (
    UndoOnDrop<u8, impl FnOnce(u8)>,
    std::sync::Arc<std::sync::atomic::AtomicU8>,
) {
    let undone = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
    let seen = std::sync::Arc::clone(&undone);
    let guard = UndoOnDrop::new(7, move |value| {
        seen.fetch_add(value, std::sync::atomic::Ordering::SeqCst);
    });
    (guard, undone)
}

#[test]
fn the_prep_guard_undoes_unless_handed_on_even_unrun_or_unwinding() {
    use std::sync::atomic::Ordering::SeqCst;
    // Handed on: no undo.
    let (guard, undone) = counted();
    assert_eq!(guard.take(), Some(7));
    assert_eq!(undone.load(SeqCst), 0);
    // A closure holding it dropped unrun (a seam thread that never started): undone once.
    let (guard, undone) = counted();
    let work = move || guard.take();
    drop(work);
    assert_eq!(undone.load(SeqCst), 7);
    // A thread holding it panics: undone once, while it unwinds.
    let (mut guard, undone) = counted();
    let panicked = std::thread::spawn(move || {
        let held = guard.get_mut().copied();
        assert_eq!(held, Some(7));
        panic!("the seam panics");
    })
    .join();
    assert!(panicked.is_err());
    assert_eq!(undone.load(SeqCst), 7);
}
