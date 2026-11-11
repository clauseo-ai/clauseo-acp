//! The session's `PATH` in the agents' shells: Codex's two `CODEX_CONFIG` keys, `path_helper`'s
//! folders on macOS, and Claude's `CLAUDE_ENV_FILE` with its one line.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::AgentKind;
use serde_json::{Map, Value, json};

use super::{
    BASH, ENV_FILE, ENV_FILE_VAR, Os, SHELL_VAR, SessionPath, apply_on, env_file_text,
    mac_shell_path, path_helper_entries, prepare_on,
};
use crate::write;

/// A fresh folder under the temp folder for `test`, removed first.
fn scratch(test: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("clauseo-shellpath-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

/// Stetdoc's folder and the Python runtime's.
fn dirs() -> Vec<PathBuf> {
    vec![PathBuf::from("/stet"), PathBuf::from("/rt/bin")]
}

/// A Codex launch's merged environment: its `CODEX_CONFIG` as the host builds it off Windows,
/// and `path` as `PATH`.
fn codex_env(path: &str) -> BTreeMap<OsString, OsString> {
    let config = super::super::codex::config_on(&BTreeMap::new(), Map::new(), None, false);
    BTreeMap::from([
        (
            OsString::from("CODEX_CONFIG"),
            OsString::from(Value::Object(config).to_string()),
        ),
        (OsString::from("PATH"), OsString::from(path)),
    ])
}

/// `env`'s `CODEX_CONFIG`, parsed.
fn codex_config(env: &BTreeMap<OsString, OsString>) -> Value {
    let text = &env[&OsString::from("CODEX_CONFIG")];
    serde_json::from_str(text.to_str().unwrap()).unwrap()
}

/// A Codex session's `SessionPath` with `dirs` and `etc_paths`.
fn codex_path(dirs: Vec<PathBuf>, etc_paths: Vec<PathBuf>) -> SessionPath {
    SessionPath {
        dirs,
        hook_path: true,
        codex: true,
        etc_paths,
        env_file: None,
        shell: None,
    }
}

/// On Linux, Codex's config gets `allow_login_shell = false` and `features.shell_snapshot =
/// false` beside R15's other features, and no `shell_environment_policy.set` (Codex then puts
/// its own folders first itself); the session's `PATH` is left as it was.
#[test]
fn codex_gets_no_login_shell_and_no_snapshot() {
    let mut env = codex_env("/stet:/rt/bin:/usr/bin:/bin");
    let before = codex_config(&env);
    apply_on(&mut env, &codex_path(dirs(), Vec::new()), Os::Linux);
    let after = codex_config(&env);
    assert_eq!(after["allow_login_shell"], json!(false));
    assert_eq!(after["features"]["shell_snapshot"], json!(false));
    assert_eq!(
        after["shell_environment_policy"],
        json!({"ignore_default_excludes": false}),
        "no set.PATH"
    );
    // Nothing else changed.
    let mut after = after;
    after.as_object_mut().unwrap().remove("allow_login_shell");
    after["features"]
        .as_object_mut()
        .unwrap()
        .remove("shell_snapshot");
    assert_eq!(after, before);
    assert_eq!(&env[&OsString::from("PATH")], "/stet:/rt/bin:/usr/bin:/bin");
    assert_eq!(env.len(), 2);
}

/// A `PATH` holding quotes, backslashes and spaces never goes in `CODEX_CONFIG`; on macOS it
/// stays in the child's environment as it is, with `path_helper`'s folders added.
#[test]
fn the_path_stays_in_the_environment() {
    let path = r#"/stet:/My "Tools"\bin:/a b:/usr/bin"#;
    let mut env = codex_env(path);
    let helper = vec![PathBuf::from("/usr/local/bin")];
    apply_on(&mut env, &codex_path(dirs(), helper), Os::Mac);
    let text = env[&OsString::from("CODEX_CONFIG")]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(!text.contains("PATH"), "{text}");
    assert_eq!(
        &env[&OsString::from("PATH")],
        r#"/stet:/rt/bin:/usr/local/bin:/My "Tools"\bin:/a b:/usr/bin"#
    );
}

/// With no folders first on `PATH`, and on Windows whatever the folders, the environment is
/// byte for byte what it was: neither key, the same `PATH`.
#[test]
fn no_folders_or_windows_leave_codex_alone() {
    let original = codex_env("/usr/bin:/bin");
    let mut env = original.clone();
    let helper = vec![PathBuf::from("/usr/local/bin")];
    apply_on(&mut env, &codex_path(Vec::new(), Vec::new()), Os::Linux);
    apply_on(&mut env, &codex_path(Vec::new(), helper.clone()), Os::Mac);
    assert_eq!(env, original);
    apply_on(&mut env, &codex_path(dirs(), helper), Os::Windows);
    assert_eq!(env, original);
    let config = codex_config(&env);
    assert!(config.get("allow_login_shell").is_none());
    assert!(config["features"].get("shell_snapshot").is_none());
}

/// A fake `/etc`: `paths`, with a blank line and a comment, and two `paths.d` files given in
/// the wrong order.
fn fake_etc(test: &str) -> PathBuf {
    let etc = scratch(test);
    write::write_file(
        &etc.join("paths"),
        b"/usr/local/bin\n/usr/bin\n\n# note\n/bin\n",
    )
    .unwrap();
    write::create_dir_all(&etc.join("paths.d")).unwrap();
    write::write_file(
        &etc.join("paths.d/20-x11"),
        b"/opt/X11/bin\n/usr/local/bin\n",
    )
    .unwrap();
    write::write_file(&etc.join("paths.d/10-homebrew"), b"/opt/homebrew/bin\n").unwrap();
    etc
}

/// `path_helper`'s folders: `paths` in order, then `paths.d` by name, without blanks and
/// comments; a missing `/etc` gives none and fails nothing.
#[test]
fn path_helper_folders_are_read_in_its_order() {
    let etc = fake_etc("helper");
    let entries = path_helper_entries(&etc);
    let expected = [
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/opt/homebrew/bin",
        "/opt/X11/bin",
    ];
    let expected: Vec<PathBuf> = expected.iter().map(PathBuf::from).collect();
    assert_eq!(
        entries,
        [expected.as_slice(), &[PathBuf::from("/usr/local/bin")]].concat()
    );
    assert_eq!(
        path_helper_entries(&etc.join("missing")),
        [] as [PathBuf; 0]
    );
    write::remove_dir_all(&etc).unwrap();
}

/// On macOS, Codex's `set.PATH` is Clauseo's folders, then `path_helper`'s, then the rest of
/// the session's `PATH`, each folder once, where it first comes.
#[test]
fn mac_codex_path_puts_path_helper_after_clauseo_and_drops_repeats() {
    let etc = fake_etc("mac");
    let run = scratch("mac-run");
    let path = prepare_on(dirs(), true, Some(AgentKind::Codex), &run, Os::Mac, &etc);
    assert_eq!(path.etc_paths.len(), 6);
    let mut env = codex_env("/stet:/rt/bin:/usr/bin:/bin:/usr/sbin:/sbin:/Users/ada/bin");
    apply_on(&mut env, &path, Os::Mac);
    let expected = "/stet:/rt/bin:/usr/local/bin:/usr/bin:/bin:/opt/homebrew/bin:/opt/X11/bin:\
                    /usr/sbin:/sbin:/Users/ada/bin";
    assert_eq!(&env[&OsString::from("PATH")], expected);
    assert_eq!(codex_config(&env)["allow_login_shell"], json!(false));
    assert!(!run.join(ENV_FILE).exists(), "Codex gets no file");
    assert!(path.shell.is_none(), "nor a shell");
    // Linux reads no `path_helper`; nor does a Codex session without folders.
    let linux = prepare_on(dirs(), true, Some(AgentKind::Codex), &run, Os::Linux, &etc);
    assert_eq!(linux.etc_paths, [] as [PathBuf; 0]);
    let none = prepare_on(
        Vec::new(),
        false,
        Some(AgentKind::Codex),
        &run,
        Os::Mac,
        &etc,
    );
    assert_eq!(none.etc_paths, [] as [PathBuf; 0]);
    write::remove_dir_all(&etc).unwrap();
    write::remove_dir_all(&run).unwrap();
}

#[test]
fn mac_shell_path_keeps_first_occurrences() {
    let helper = [PathBuf::from("/usr/local/bin"), PathBuf::from("/stet")];
    assert_eq!(
        mac_shell_path(&dirs(), &helper, "/usr/bin::/stet:/usr/bin"),
        "/stet:/rt/bin:/usr/local/bin:/usr/bin"
    );
}

/// Claude's file is one line: the folders inside single quotes, a `'` written `'\''`, then
/// `"$PATH"`; `CLAUDE_ENV_FILE` names it, over an inherited one of any case.
#[test]
fn claude_gets_its_env_file_with_quoted_folders() {
    let run = scratch("claude");
    let dirs = vec![
        PathBuf::from("/Users/ada/Clauseo Data/rt/bin"),
        PathBuf::from("/opt/o'brien/bin"),
    ];
    let path = prepare_on(
        dirs,
        true,
        Some(AgentKind::Claude),
        &run,
        Os::Mac,
        Path::new("/nowhere"),
    );
    let file = run.join(ENV_FILE);
    assert_eq!(path.env_file.as_deref(), Some(file.as_path()));
    // Claude's snapshot keeps path_helper's folders.
    assert_eq!(path.etc_paths, [] as [PathBuf; 0]);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "export PATH='/Users/ada/Clauseo Data/rt/bin:/opt/o'\\''brien/bin':\"$PATH\"\n"
    );
    let mut env = BTreeMap::from([
        (
            OsString::from("claude_env_file"),
            OsString::from("/Users/ada/mine.sh"),
        ),
        (OsString::from("PATH"), OsString::from("/usr/bin")),
    ]);
    apply_on(&mut env, &path, Os::Mac);
    assert_eq!(
        env.get(&OsString::from(ENV_FILE_VAR)),
        Some(&file.into_os_string())
    );
    assert!(!env.contains_key(&OsString::from("claude_env_file")));
    assert_eq!(
        env.get(&OsString::from(SHELL_VAR)),
        Some(&OsString::from(BASH))
    );
    assert_eq!(env.len(), 3);
    write::remove_dir_all(&run).unwrap();
}

/// With no folders, a file left in the run folder is removed, no `CLAUDE_ENV_FILE` is set and
/// an inherited one stays as it was; `CLAUDE_CODE_SHELL` is still bash, over an inherited one,
/// on Linux when `/bin/bash` is there.
#[test]
fn claude_without_folders_gets_no_file_and_keeps_an_inherited_one() {
    let run = scratch("claude-none");
    let file = run.join(ENV_FILE);
    write::write_file(&file, b"export PATH='/old':\"$PATH\"\n").unwrap();
    let path = prepare_on(
        Vec::new(),
        false,
        Some(AgentKind::Claude),
        &run,
        Os::Linux,
        Path::new("/nowhere"),
    );
    assert!(path.env_file.is_none());
    assert!(!file.exists(), "the old file is gone");
    let bash = Path::new(BASH).is_file().then(|| PathBuf::from(BASH));
    assert_eq!(path.shell, bash);
    let original = BTreeMap::from([(
        OsString::from(ENV_FILE_VAR),
        OsString::from("/Users/ada/mine.sh"),
    )]);
    let mut env = original.clone();
    env.insert(
        OsString::from("claude_code_shell"),
        OsString::from("/bin/zsh"),
    );
    let path = SessionPath {
        shell: Some(PathBuf::from(BASH)),
        ..path
    };
    apply_on(&mut env, &path, Os::Linux);
    let mut want = original;
    want.insert(OsString::from(SHELL_VAR), OsString::from(BASH));
    assert_eq!(env, want);
    write::remove_dir_all(&run).unwrap();
}

/// On Windows a Claude session gets no file and no variable, whatever its folders.
#[test]
fn windows_claude_is_left_alone() {
    let run = scratch("claude-windows");
    let path = prepare_on(
        dirs(),
        true,
        Some(AgentKind::Claude),
        &run,
        Os::Windows,
        Path::new("/nowhere"),
    );
    assert!(path.env_file.is_none() && path.shell.is_none());
    assert!(!run.join(ENV_FILE).exists());
    let mut env = BTreeMap::new();
    let with_file = SessionPath {
        env_file: Some(run.join(ENV_FILE)),
        shell: Some(PathBuf::from(BASH)),
        ..path
    };
    apply_on(&mut env, &with_file, Os::Windows);
    assert_eq!(env, BTreeMap::new());
    write::remove_dir_all(&run).unwrap();
}

/// A folder holding `:` or a line break would break the line; it is left out of the file.
#[test]
fn unusable_folders_are_left_out_of_the_line() {
    let dirs = [
        PathBuf::from("/a:b"),
        PathBuf::from("/c\nd"),
        PathBuf::from("/ok"),
    ];
    assert_eq!(
        env_file_text(&dirs).as_deref(),
        Some("export PATH='/ok':\"$PATH\"\n")
    );
    assert_eq!(env_file_text(&dirs[..2]), None);
}

/// The file is the user's own: 0600.
#[cfg(unix)]
#[test]
fn the_env_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let run = scratch("claude-mode");
    let path = prepare_on(
        dirs(),
        true,
        Some(AgentKind::Claude),
        &run,
        Os::Linux,
        Path::new("/nowhere"),
    );
    let mode = std::fs::metadata(path.env_file.unwrap())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    write::remove_dir_all(&run).unwrap();
}

/// A folder named with a command substitution, a backtick and `$HOME` stays inert when `sh`
/// sources the file: nothing runs, and the literal folder comes back first on `PATH`. The
/// shell runs through the host's own launcher (`clauseo_process::spawn_in_tree`).
#[cfg(unix)]
#[test]
fn sourcing_the_env_file_runs_nothing() {
    use clauseo_process::{LaunchSpec, Launched, SpawnOptions, spawn_in_tree};
    use futures::AsyncReadExt as _;

    let run = scratch("claude-inert");
    let odd = run.join("a $(touch PWNED) `touch TICKED` $HOME 'q'");
    let path = prepare_on(
        vec![odd.clone()],
        true,
        Some(AgentKind::Claude),
        &run,
        Os::Linux,
        Path::new("/nowhere"),
    );
    let file = path.env_file.unwrap();
    let env = BTreeMap::from([
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
        (OsString::from("HOME"), OsString::from("/home/nobody")),
    ]);
    let args = vec![
        OsString::from("-c"),
        OsString::from(". \"$1\" && printf %s \"$PATH\""),
        OsString::from("sh"),
        file.into_os_string(),
    ];
    let spec = LaunchSpec::new(PathBuf::from("/bin/sh"), args, env, run.clone()).unwrap();
    let Launched {
        mut handle,
        mut stdout,
        stderr,
        ..
    } = spawn_in_tree(&spec, &SpawnOptions::new().clear_env()).unwrap();
    let (said, report) = futures::executor::block_on(async {
        let mut said = Vec::new();
        let (read, _drained) = futures::join!(stdout.read_to_end(&mut said), stderr.drain());
        read.unwrap();
        (said, handle.exited().await)
    });
    assert_eq!(
        report.code(),
        Some(0),
        "sh failed: {}",
        report.stderr_tail()
    );
    let expected = format!("{}:/usr/bin:/bin", odd.to_str().unwrap());
    assert_eq!(String::from_utf8(said).unwrap(), expected);
    assert!(!run.join("PWNED").exists(), "the substitution ran");
    assert!(!run.join("TICKED").exists(), "the backtick ran");
    write::remove_dir_all(&run).unwrap();
}
