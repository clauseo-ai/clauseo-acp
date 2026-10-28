use super::*;

/// An absolute path on every OS: the temp folder is absolute, and on Windows it has a drive.
fn absolute(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

fn spec(program: PathBuf, cwd: PathBuf) -> Result<LaunchSpec, Error> {
    LaunchSpec::new(program, Vec::new(), BTreeMap::new(), cwd)
}

fn refused(result: Result<LaunchSpec, Error>) -> String {
    match result {
        Err(Error::InvalidSpec(why)) => why,
        other => panic!("expected InvalidSpec, got {other:?}"),
    }
}

#[test]
fn absolute_program_in_a_folder_is_valid() {
    let env = BTreeMap::from([(OsString::from("A"), OsString::from("1"))]);
    let args = vec![OsString::from("--acp")];
    let spec = LaunchSpec::new(
        absolute("agent"),
        args.clone(),
        env.clone(),
        std::env::temp_dir(),
    )
    .unwrap();
    assert_eq!(spec.program(), absolute("agent"));
    assert_eq!(spec.args(), args);
    assert_eq!(spec.env(), &env);
    assert_eq!(spec.cwd(), std::env::temp_dir());
}

#[test]
fn relative_program_is_invalid() {
    for program in ["agent", "bin/agent", "./agent"] {
        let why = refused(spec(PathBuf::from(program), std::env::temp_dir()));
        assert!(why.contains("not an absolute path"), "{program}: {why}");
    }
}

#[test]
fn cmd_and_bat_refused() {
    for name in [
        "claude.cmd",
        "claude.CMD",
        "codex.Bat",
        "codex.bat",
        "codex.bat.",
        "codex.cmd .",
        ".cmd",
    ] {
        let why = refused(spec(absolute(name), std::env::temp_dir()));
        assert!(why.contains("is a batch file"), "{name}: {why}");
    }
}

#[test]
fn names_close_to_a_batch_file_are_programs() {
    for name in [
        "claude.exe",
        "cmd",
        "bat",
        "agent.cmdx",
        "agent.bash",
        "a.cm",
        ".",
        "md",
    ] {
        assert!(spec(absolute(name), std::env::temp_dir()).is_ok(), "{name}");
    }
}

#[test]
fn relative_cwd_is_invalid() {
    let why = refused(spec(absolute("agent"), PathBuf::from("matter")));
    assert!(why.contains("working folder"), "{why}");
    assert!(why.contains("not an absolute path"), "{why}");
}

#[test]
fn cwd_must_be_a_folder() {
    let file = std::env::current_exe().unwrap();
    let why = refused(spec(absolute("agent"), file));
    assert!(why.ends_with("is not a folder"), "{why}");
    let missing = absolute("clauseo-process-no-such-folder");
    let why = refused(spec(absolute("agent"), missing));
    assert!(why.ends_with("is not a folder"), "{why}");
}
