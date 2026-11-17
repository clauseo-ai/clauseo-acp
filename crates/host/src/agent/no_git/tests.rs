//! The `git` that does nothing: the launch prep's check of the program, the variables a launch
//! gets, and a Codex session's shell `PATH`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentKind, ErrorCode};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::{
    ATTR_SOURCE_VAR, CEILING_VAR, EMPTY_TREE, Unusable, check, codex_shell_path, launch_env,
    refusal,
};
use crate::config::NoGit;
use crate::write;

/// The bytes the tests' program holds.
const PROGRAM: &[u8] = b"the program's bytes";

/// A fresh folder under the temp folder for `test`, removed first.
fn scratch(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("clauseo-nogit-{test}-{}", std::process::id()));
    drop(write::remove_dir_all(&root));
    write::create_dir_all(&root).unwrap();
    root
}

/// `<root>/tools/no-git/git` holding [`PROGRAM`], and the [`NoGit`] that expects it.
fn installed(root: &Path) -> NoGit {
    let folder = root.join("tools").join("no-git");
    write::create_dir_all(&folder).unwrap();
    let program = folder.join(format!("git{}", std::env::consts::EXE_SUFFIX));
    write::write_file(&program, PROGRAM).unwrap();
    NoGit::new(program, Some(Sha256::digest(PROGRAM).into()))
}

#[test]
fn a_sound_program_gives_its_folder() {
    let root = scratch("sound");
    let no_git = installed(&root);
    assert_eq!(check(&no_git), Ok(root.join("tools").join("no-git")));
    drop(write::remove_dir_all(&root));
}

#[test]
fn a_vanished_program_is_refused_with_its_own_code() {
    let root = scratch("vanished");
    let no_git = installed(&root);
    // What a virus scanner's quarantine leaves: nothing at the name.
    write::remove_file(&no_git.program).unwrap();
    assert_eq!(check(&no_git), Err(Unusable::Missing));
    let error = refusal(Unusable::Missing);
    assert_eq!(error.code(), ErrorCode::AgentToolMissing);
    assert!(!error.to_string().contains(&*root.to_string_lossy()));
    // The lawyer-facing text is the neutral sentence; the reason is `check`'s answer above.
    let text = error.to_string();
    assert!(
        text.contains("A file Clauseo keeps for the agent isn't as Clauseo left it."),
        "{text}"
    );
    assert!(!text.contains("PATH"), "{text}");
    drop(write::remove_dir_all(&root));
}

#[test]
fn a_changed_a_shorter_or_a_never_copied_program_is_refused() {
    let root = scratch("changed");
    let no_git = installed(&root);
    write::write_file(&no_git.program, b"other bytes of the same kind").unwrap();
    assert_eq!(check(&no_git), Err(Unusable::Changed));
    write::write_file(&no_git.program, &PROGRAM[..4]).unwrap();
    assert_eq!(check(&no_git), Err(Unusable::Changed));
    write::write_file(&no_git.program, b"").unwrap();
    assert_eq!(check(&no_git), Err(Unusable::Changed));
    let never = NoGit::new(no_git.program.clone(), None);
    write::write_file(&no_git.program, PROGRAM).unwrap();
    assert_eq!(check(&never), Err(Unusable::NotCopied));
    drop(write::remove_dir_all(&root));
}

#[test]
fn a_folder_at_the_programs_name_is_refused() {
    let root = scratch("folder");
    let no_git = installed(&root);
    write::remove_file(&no_git.program).unwrap();
    write::create_dir_all(&no_git.program).unwrap();
    assert_eq!(check(&no_git), Err(Unusable::NotAFile));
    drop(write::remove_dir_all(&root));
}

#[cfg(unix)]
#[test]
fn a_link_to_the_same_bytes_is_refused() {
    let root = scratch("link");
    let no_git = installed(&root);
    let elsewhere = root.join("elsewhere");
    write::write_file(&elsewhere, PROGRAM).unwrap();
    write::remove_file(&no_git.program).unwrap();
    write::symlink(&elsewhere, &no_git.program).unwrap();
    assert_eq!(check(&no_git), Err(Unusable::NotAFile));
    drop(write::remove_dir_all(&root));
}

#[test]
fn both_agents_get_the_attribute_source_and_codex_alone_the_ceiling() {
    let matter = std::env::temp_dir().join("matters").join("Smith v Jones");
    let claude = launch_env(Some(AgentKind::Claude), &matter);
    assert_eq!(
        claude,
        BTreeMap::from([(OsString::from(ATTR_SOURCE_VAR), OsString::from(EMPTY_TREE))])
    );
    let codex = launch_env(Some(AgentKind::Codex), &matter);
    assert_eq!(codex.len(), 2);
    assert_eq!(
        codex.get(&OsString::from(ATTR_SOURCE_VAR)),
        Some(&OsString::from(EMPTY_TREE))
    );
    assert_eq!(
        codex.get(&OsString::from(CEILING_VAR)),
        Some(&std::env::temp_dir().join("matters").into_os_string())
    );
    assert!(launch_env(None, &matter).is_empty());
}

/// A Codex launch's environment: a `CODEX_CONFIG` with one `shell_environment_policy` key, and
/// `path` as `PATH`.
fn codex_env(path: &str) -> BTreeMap<OsString, OsString> {
    let config = json!({"shell_environment_policy": {"ignore_default_excludes": false}});
    BTreeMap::from([
        (
            OsString::from("CODEX_CONFIG"),
            OsString::from(config.to_string()),
        ),
        (OsString::from("PATH"), OsString::from(path)),
    ])
}

/// The `CODEX_CONFIG` in `env`, parsed.
fn config(env: &BTreeMap<OsString, OsString>) -> Value {
    let text = env.get(&OsString::from("CODEX_CONFIG")).unwrap();
    serde_json::from_str(text.to_str().unwrap()).unwrap()
}

#[test]
fn codex_shells_get_the_sessions_whole_path_with_the_program_first() {
    let path = "/data/tools/no-git:/stet:/usr/bin";
    let mut env = codex_env(path);
    codex_shell_path(&mut env, true);
    assert_eq!(
        config(&env)["shell_environment_policy"],
        json!({"ignore_default_excludes": false, "set": {"PATH": path}})
    );
    let mut without = codex_env(path);
    codex_shell_path(&mut without, false);
    assert_eq!(without, codex_env(path));
}

/// The program sits where no agent may write: under `<data>/tools`, which Claude's rules make
/// read-only (`data_fence::READ_ONLY`, an `Edit(<data>/tools/**)` deny, pinned in
/// `data_fence/launch_tests.rs`) and Codex's sandbox reads but never writes.
#[test]
fn the_program_sits_where_no_agent_may_write() {
    use crate::agent::codex_fence::tests::{access, data, fence};

    assert!(crate::agent::data_fence::READ_ONLY.contains(&"tools"));
    let data = data();
    let program = clauseo_profile::paths::no_git_command(&data);
    assert!(program.starts_with(data.join("tools")));
    let keys = fence();
    for path in [program.as_path(), program.parent().unwrap()] {
        assert_eq!(access(&keys, path), "read", "{}", path.display());
    }
}

/// On Windows Claude Code looks for Git Bash two folders above the `git.exe` it finds:
/// `<data>/tools/bin/bash.exe`, inside the same read-only folder, which Clauseo never makes.
#[test]
fn two_folders_above_the_program_holds_no_bash() {
    let data = std::env::temp_dir().join("clauseo-nogit-bash").join("data");
    let program = clauseo_profile::paths::no_git_command(&data);
    let bash = program
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("bin")
        .join("bash.exe");
    assert_eq!(bash, data.join("tools").join("bin").join("bash.exe"));
    assert!(!bash.exists());
}

#[test]
fn anything_beside_the_program_but_a_dot_leftover_is_refused() {
    let root = scratch("crowded");
    let no_git = installed(&root);
    let folder = no_git.program.parent().unwrap().to_path_buf();
    write::write_file(&folder.join(".git.exe.7-0.old"), b"old copy").unwrap();
    assert_eq!(check(&no_git), Ok(folder.clone()));
    // `GIT` beside `git` only where names keep their case (on macOS it is the program itself).
    let cased: &[&str] = if cfg!(target_os = "linux") {
        &["GIT"]
    } else {
        &[]
    };
    for beside in ["git.cmd", "git.com", "git.bat", "notes.txt"]
        .iter()
        .chain(cased)
    {
        let path = folder.join(beside);
        write::write_file(&path, b"x").unwrap();
        assert_eq!(check(&no_git), Err(Unusable::Crowded), "{beside}");
        write::remove_file(&path).unwrap();
    }
    write::create_dir_all(&folder.join("bin")).unwrap();
    assert_eq!(check(&no_git), Err(Unusable::Crowded));
    drop(write::remove_dir_all(&root));
}

/// On macOS and Windows the program in another case (`GIT.EXE`, which the desktop's sweep keeps
/// and finds up to date) is the program, never "crowded"; where names keep their case it is
/// another file.
#[test]
fn the_programs_own_name_in_another_case_counts_where_disks_fold_case() {
    use std::ffi::OsStr;

    use super::same_name;

    assert!(same_name(
        OsStr::new("GIT.EXE"),
        OsStr::new("git.exe"),
        true
    ));
    assert!(same_name(OsStr::new("Git"), OsStr::new("git"), true));
    assert!(!same_name(
        OsStr::new("GIT.EXE"),
        OsStr::new("git.exe"),
        false
    ));
    assert!(!same_name(
        OsStr::new("git.cmd"),
        OsStr::new("git.exe"),
        true
    ));
    assert_eq!(super::FOLD_CASE, cfg!(any(windows, target_os = "macos")));
}

#[test]
fn a_build_for_the_lawyer_refuses_a_launch_with_no_program_named() {
    use super::required;

    let no_git = NoGit::new(PathBuf::from("/data/tools/no-git/git"), None);
    assert_eq!(required(None, false), Err(Unusable::NotWired));
    assert_eq!(required(None, true), Ok(None));
    assert_eq!(required(Some(&no_git), false), Ok(Some(&no_git)));
    const { assert!(super::TEST_BUILD, "unit tests are a test build") };
}

/// A Codex install laid out as `@openai/codex` 0.162.1's Windows package is, under `root`:
/// its `vendor/<triple>` folder, holding `bin` (codex.exe's folder) and `codex-path`.
fn codex_install(root: &Path) -> (PathBuf, PathBuf) {
    let install = root
        .join("agents")
        .join("codex-acp")
        .join("install")
        .join("ab12");
    let openai = install.join("node_modules").join("@openai");
    write::create_dir_all(&openai.join("codex").join("bin")).unwrap();
    write::write_file(&openai.join("codex").join("bin").join("codex.js"), b"js").unwrap();
    let triple = openai
        .join("codex-win32-x64")
        .join("vendor")
        .join("x86_64-pc-windows-msvc");
    for inner in ["bin", "codex-path"] {
        write::create_dir_all(&triple.join(inner)).unwrap();
    }
    write::write_file(&triple.join("bin").join("codex.exe"), b"codex").unwrap();
    write::write_file(&triple.join("codex-path").join("rg.exe"), b"rg").unwrap();
    (install, triple)
}

#[test]
fn a_git_program_in_a_codex_install_folder_is_refused() {
    use super::codex_install::{Beside, beside_codex};
    use super::{Check, check_all};

    let root = scratch("beside-codex");
    let no_git = installed(&root);
    let (install, triple) = codex_install(&root);
    let check = Check {
        no_git,
        codex_install: Some(install.clone()),
    };
    assert_eq!(beside_codex(&install), Ok(()));
    assert!(check_all(&check).is_ok());
    for (folder, name) in [
        ("bin", "git.exe"),
        ("bin", "GIT.COM"),
        ("codex-path", "git.cmd"),
        ("codex-path", "git.bat"),
        ("", "git.exe"),
    ] {
        let at = if folder.is_empty() {
            triple.clone()
        } else {
            triple.join(folder)
        };
        let path = at.join(name);
        write::write_file(&path, b"x").unwrap();
        assert_eq!(beside_codex(&install), Err(Beside::Git), "{folder}/{name}");
        assert_eq!(
            check_all(&check),
            Err(Unusable::BesideCodex),
            "{folder}/{name}"
        );
        write::remove_file(&path).unwrap();
    }
    write::write_file(&triple.join("bin").join("gitk.exe"), b"x").unwrap();
    assert_eq!(beside_codex(&install), Ok(()));
    drop(write::remove_dir_all(&root));
}

/// The scan fails closed: no `vendor/<triple>` at all, a folder it can't read, or a link
/// among the entries is refused like a git program.
#[test]
fn a_codex_install_the_scan_cannot_vouch_for_is_refused() {
    use super::codex_install::{Beside, beside_codex};

    let root = scratch("codex-unvouched");
    let bare = root.join("bare");
    write::create_dir_all(&bare.join("node_modules").join("@openai").join("codex")).unwrap();
    assert_eq!(beside_codex(&bare), Err(Beside::NoVendor));
    assert_eq!(beside_codex(&root.join("absent")), Err(Beside::Unreadable));
    #[cfg(unix)]
    {
        let (install, triple) = codex_install(&root);
        write::symlink(
            &root.join("elsewhere"),
            &triple.join("codex-path").join("tool"),
        )
        .unwrap();
        assert_eq!(beside_codex(&install), Err(Beside::Link));
    }
    drop(write::remove_dir_all(&root));
}

/// A `vendor` (or `node_modules`, `@openai`) that is itself a link or a junction is refused,
/// whatever lies beyond it.
#[test]
#[cfg_attr(
    windows,
    expect(
        clippy::disallowed_methods,
        reason = "the test makes its junction with cmd's mklink /J, which exits at once"
    )
)]
fn a_linked_vendor_folder_is_refused() {
    use super::codex_install::{Beside, beside_codex};

    let root = scratch("codex-linked-vendor");
    let (install, triple) = codex_install(&root);
    let vendor = triple.parent().unwrap().to_path_buf();
    let outside = root.join("outside");
    write::create_dir_all(&outside).unwrap();
    write::rename(&vendor, &outside.join("vendor")).unwrap();
    let target = outside.join("vendor");
    #[cfg(unix)]
    write::symlink(&target, &vendor).unwrap();
    #[cfg(windows)]
    {
        let made = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(&vendor)
            .arg(&target)
            .status()
            .unwrap();
        assert!(made.success());
    }
    assert_eq!(beside_codex(&install), Err(Beside::Link));
    drop(write::remove_dir_all(&root));
}

/// A git name above `vendor/<triple>` (in `@openai` or `vendor` itself) is no folder Rust or
/// codex.exe searches, nor one the start-up removes from: not refused.
#[test]
fn a_git_name_above_the_searched_folders_is_not_refused() {
    use super::codex_install::beside_codex;

    let root = scratch("codex-above");
    let (install, triple) = codex_install(&root);
    let vendor = triple.parent().unwrap().to_path_buf();
    let openai = vendor.parent().unwrap().parent().unwrap().to_path_buf();
    write::write_file(&openai.join("git.exe"), b"x").unwrap();
    write::write_file(&vendor.join("git.cmd"), b"x").unwrap();
    assert_eq!(beside_codex(&install), Ok(()));
    drop(write::remove_dir_all(&root));
}

/// A junction in a searched folder is refused as a link: Rust's `is_symlink` reports it.
#[cfg(windows)]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test makes its junction with cmd's mklink /J, which exits at once"
)]
fn a_junction_in_a_codex_install_folder_is_refused() {
    use super::codex_install::{Beside, beside_codex};

    let root = scratch("codex-junction");
    let (install, triple) = codex_install(&root);
    let elsewhere = root.join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    let made = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(triple.join("codex-path").join("tools"))
        .arg(&elsewhere)
        .status()
        .unwrap();
    assert!(made.success());
    assert_eq!(beside_codex(&install), Err(Beside::Link));
    drop(write::remove_dir_all(&root));
}

#[test]
fn the_codex_install_is_found_from_the_adapters_script() {
    use super::codex_install::install_of;
    use clauseo_settings::AgentLaunch;

    let install = std::env::temp_dir()
        .join("agents")
        .join("codex-acp")
        .join("install")
        .join("ab12");
    let script = install
        .join("node_modules")
        .join("@zed-industries")
        .join("codex-acp")
        .join("bin")
        .join("codex-acp.js");
    let node = std::env::temp_dir().join("runtime").join("node");
    let agent = |command: &Path, args: Vec<String>| AgentLaunch {
        command: command.to_str().unwrap().to_owned(),
        args,
        env: BTreeMap::new(),
    };
    let entry = agent(&node, vec![script.to_str().unwrap().to_owned()]);
    assert_eq!(install_of(&entry), Some(install));
    assert_eq!(install_of(&agent(&node, Vec::new())), None);
    let relative = agent(&node, vec!["node_modules/x/bin.js".to_owned()]);
    assert_eq!(install_of(&relative), None);
}

/// What a launch and each call check: nothing for an agent with no kind; a refusal with no
/// program named in a build for the lawyer; and a Codex agent's install folder only on Windows.
#[test]
fn each_agent_gets_its_check_and_none_without_a_program_is_refused_for_the_lawyer() {
    use clauseo_settings::AgentLaunch;

    use super::check_for;

    let program = NoGit::new(PathBuf::from("/data/tools/no-git/git"), Some([7; 32]));
    let install = std::env::temp_dir().join("install");
    let script = install
        .join("node_modules")
        .join("codex-acp")
        .join("bin.js");
    let entry = AgentLaunch {
        command: "/runtime/node".to_owned(),
        args: vec![script.to_str().unwrap().to_owned()],
        env: BTreeMap::new(),
    };
    let codex = Some(AgentKind::Codex);
    let lawyer = (false, false);
    assert_eq!(
        check_for(codex, None, Some(&entry), lawyer).err(),
        Some(Unusable::NotWired)
    );
    let claude = Some(AgentKind::Claude);
    assert_eq!(
        check_for(claude, None, None, (false, true)).err(),
        Some(Unusable::NotWired)
    );
    assert!(matches!(check_for(None, None, None, lawyer), Ok(None)));
    assert!(matches!(
        check_for(codex, None, None, (true, true)),
        Ok(None)
    ));
    let on = |kind, windows| {
        check_for(kind, Some(&program), Some(&entry), (false, windows))
            .unwrap()
            .unwrap()
    };
    assert_eq!(on(codex, true).codex_install, Some(install));
    assert_eq!(on(codex, false).codex_install, None);
    assert_eq!(on(claude, true).codex_install, None);
    assert_eq!(on(claude, true).no_git, program);
}
