//! The launch's self-test of Codex's guard hook, run as Codex runs it: the program and arguments
//! for each OS, from the configured hook line and the shell Codex picks; a guard that is missing,
//! lets the unknown session's call go, says nothing, hangs past the limit, or refuses the
//! conversation's own call fails it; one that refuses the first and allows the second passes, and
//! so does the real guard with the session listed (this test binary run again as the guard, on
//! Unix), but not with an ids file it can't read.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

#[cfg(unix)]
use serde_json::json;
use serde_json::{Map, Value};

use super::{Failed, Host, LIMIT, Os, argv_on, command_of, input, limit_for, run, run_within};
#[cfg(unix)]
use super::{Kind, SelfTest, allows_within, argv, kind_of, own_input};
use crate::write;

/// A scratch folder, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "clauseo-host-self-test-{test}-{}",
            std::process::id()
        ));
        write::remove_dir_all(&root).unwrap();
        write::create_dir_all(&root).unwrap();
        Self(root)
    }

    /// A plain environment.
    fn env() -> BTreeMap<OsString, OsString> {
        let mut env = BTreeMap::new();
        for name in ["PATH", "SystemRoot", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(name) {
                env.insert(OsString::from(name), value);
            }
        }
        env
    }

    /// Runs the hook `entry` here, within `limit`.
    fn run(&self, entry: &Map<String, Value>, limit: Duration) -> Result<(), Failed> {
        run_within(Some(entry), (&input(&self.0), &self.0, &Self::env()), limit)
    }

    /// The self-test of the hook `entry` here.
    #[cfg(unix)]
    fn test(&self, entry: Map<String, Value>) -> SelfTest {
        SelfTest {
            entry,
            env: Self::env(),
            cwd: self.0.clone(),
        }
    }

    /// A guard script holding `body`; its path.
    #[cfg(unix)]
    fn guard(&self, body: &str) -> PathBuf {
        let path = self.0.join("guard.sh");
        write::write_file(&path, format!("#!/bin/sh\n{body}\n").as_bytes()).unwrap();
        write::set_mode(&path, 0o755).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}

/// Codex's hooks object for the guard program `program`, as the host writes it.
fn entry_for(program: &Path) -> Map<String, Value> {
    let config = crate::agent::loads::codex_hook_config(program, None, &[]).unwrap();
    config.as_object().unwrap().clone()
}

/// `text` in POSIX single quotes.
#[cfg(unix)]
fn posix_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The argv for each OS is Codex's for the configured line itself: elsewhere the login shell
/// with `-c <command>` (`the_shell_off_windows_is_the_one_codex_picks`); on Windows PowerShell
/// with `-NoProfile -Command <commandWindows>`, picked as Codex picks it, step by step (no file
/// system: `exists` is given), and `cmd.exe /c` with the line raw in quotes when there is none.
#[test]
fn the_argv_is_codexs_for_the_configured_line_on_each_os() {
    let config = crate::agent::loads::codex_hook_config(
        Path::new(&crate::testing::abs("/opt/g d/x")),
        None,
        &[],
    )
    .unwrap();
    let handler = &config["PreToolUse"][0]["hooks"][0];
    let posix = handler["command"].as_str().unwrap();
    let windows = handler["commandWindows"].as_str().unwrap();
    let env = |pairs: &[(&str, OsString)]| -> BTreeMap<OsString, OsString> {
        pairs
            .iter()
            .map(|(key, value)| (OsString::from(key), value.clone()))
            .collect()
    };
    let zsh = |path: &Path| path == Path::new("/bin/zsh");
    let host = Host {
        os: Os::Mac,
        login: Some(PathBuf::from("/bin/zsh")),
        exists: &zsh,
    };
    assert_eq!(
        argv_on(posix, &env(&[]), &host),
        (
            PathBuf::from("/bin/zsh"),
            vec![OsString::from("-c"), OsString::from(posix)]
        )
    );

    let powershell_args = vec![
        OsString::from("-NoProfile"),
        OsString::from("-Command"),
        OsString::from(windows),
    ];
    let (first, second) = (PathBuf::from("first"), PathBuf::from("second"));
    let path = std::env::join_paths([&first, &second]).unwrap();
    let path_env = env(&[("Path", path)]);
    let program_files = PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe");
    let system = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
    let on_windows = |exists: &dyn Fn(&Path) -> bool, env: &BTreeMap<OsString, OsString>| {
        let host = Host {
            os: Os::Windows,
            login: None,
            exists,
        };
        argv_on(windows, env, &host)
    };
    let pick = |there: &[PathBuf]| {
        let exists = |path: &Path| there.iter().any(|file| file == path);
        on_windows(&exists, &path_env)
    };
    // 1. pwsh on PATH, wherever powershell is.
    let all = [
        first.join("powershell.exe"),
        second.join("pwsh.exe"),
        program_files.clone(),
        system.clone(),
    ];
    assert_eq!(
        pick(&all),
        (second.join("pwsh.exe"), powershell_args.clone())
    );
    // 2. Then Program Files' pwsh 7.
    assert_eq!(
        pick(
            &all[0..1]
                .iter()
                .chain(&all[2..])
                .cloned()
                .collect::<Vec<_>>()
        )
        .0,
        program_files
    );
    // 3. Then powershell on PATH.
    assert_eq!(
        pick(&[first.join("powershell.exe"), system.clone()]).0,
        first.join("powershell.exe")
    );
    // 4. Then the system's Windows PowerShell.
    assert_eq!(
        pick(std::slice::from_ref(&system)),
        (system, powershell_args)
    );
    // None: cmd.exe, the line raw in quotes after `/c`.
    assert_eq!(
        pick(&[]),
        (
            PathBuf::from("cmd.exe"),
            vec![
                OsString::from("/c"),
                OsString::from(format!("\"{windows}\""))
            ]
        )
    );
    // A Store PowerShell on PATH is kept: the session's shell, which hooks run under, has no
    // Store-path skip (that is for sandboxed commands).
    let store = PathBuf::from("WindowsApps").join("pwsh.exe");
    let store_path = std::env::join_paths([PathBuf::from("WindowsApps")]).unwrap();
    let exists = |path: &Path| path == store;
    assert_eq!(on_windows(&exists, &env(&[("PATH", store_path)])).0, store);
}

/// Off Windows the self-test's shell is the one Codex 0.162.1 picks (`shell_detect.rs`,
/// `default_user_shell_from_path`): the account's login shell from the user database, never
/// `SHELL`, when Codex knows its kind; else zsh then bash on macOS, bash then zsh elsewhere;
/// else `/bin/sh`; each kind found as the login shell itself, then on `PATH`, then its fixed
/// paths; run non-login with `-c` (PowerShell `-NoProfile -Command`). Unix paths throughout.
#[cfg(unix)]
#[test]
fn the_shell_off_windows_is_the_one_codex_picks() {
    let line = "'/opt/g' --pre-write-guard";
    let c = vec![OsString::from("-c"), OsString::from(line)];
    let bin = PathBuf::from("bin");
    let env: BTreeMap<OsString, OsString> = [
        (OsString::from("SHELL"), OsString::from("/usr/bin/fish")),
        (
            OsString::from("PATH"),
            std::env::join_paths([&bin]).unwrap(),
        ),
    ]
    .into_iter()
    .collect();
    let pick = |os: Os, login: Option<&str>, there: &[PathBuf]| {
        let exists = |path: &Path| there.iter().any(|file| file == path);
        let host = Host {
            os,
            login: login.map(PathBuf::from),
            exists: &exists,
        };
        argv_on(line, &env, &host)
    };
    let all = [
        PathBuf::from("/bin/zsh"),
        PathBuf::from("/bin/bash"),
        PathBuf::from("/bin/sh"),
        PathBuf::from("/usr/bin/fish"),
        PathBuf::from("/opt/homebrew/bin/bash"),
    ];
    // The login shell, not `SHELL`.
    for os in [Os::Mac, Os::Unix] {
        assert_eq!(
            pick(os, Some("/opt/homebrew/bin/bash"), &all),
            (PathBuf::from("/opt/homebrew/bin/bash"), c.clone())
        );
        assert_eq!(pick(os, Some("/bin/sh"), &all).0, PathBuf::from("/bin/sh"));
    }
    // A login shell Codex does not know: zsh first on macOS, bash first elsewhere.
    assert_eq!(
        pick(Os::Mac, Some("/usr/bin/fish"), &all),
        (PathBuf::from("/bin/zsh"), c.clone())
    );
    assert_eq!(
        pick(Os::Unix, Some("/usr/bin/fish"), &all).0,
        PathBuf::from("/bin/bash")
    );
    assert_eq!(pick(Os::Unix, None, &all).0, PathBuf::from("/bin/bash"));
    // Then the other one.
    let no_bash = [PathBuf::from("/bin/zsh")];
    assert_eq!(pick(Os::Unix, None, &no_bash).0, PathBuf::from("/bin/zsh"));
    let no_zsh = [PathBuf::from("/usr/bin/bash")];
    assert_eq!(
        pick(Os::Mac, None, &no_zsh).0,
        PathBuf::from("/usr/bin/bash")
    );
    // A known login shell that is not there: its kind on `PATH`, then at its fixed path.
    let on_path = [bin.join("zsh"), PathBuf::from("/bin/zsh")];
    assert_eq!(
        pick(Os::Unix, Some("/usr/local/bin/zsh"), &on_path).0,
        bin.join("zsh")
    );
    assert_eq!(
        pick(Os::Unix, Some("/usr/local/bin/zsh"), &on_path[1..]).0,
        PathBuf::from("/bin/zsh")
    );
    // Nothing found: `/bin/sh`, still with `-c`.
    assert_eq!(
        pick(Os::Mac, Some("/usr/bin/fish"), &[]),
        (PathBuf::from("/bin/sh"), c)
    );
    // A PowerShell login shell runs the line as Codex runs it there.
    let pwsh = [PathBuf::from("/usr/local/bin/pwsh")];
    assert_eq!(
        pick(Os::Mac, Some("/usr/local/bin/pwsh"), &pwsh),
        (
            PathBuf::from("/usr/local/bin/pwsh"),
            ["-NoProfile", "-Command", line]
                .map(OsString::from)
                .to_vec()
        )
    );
    // Codex's `detect_shell_type`: the whole text, else its file stem, again.
    for (path, kind) in [
        ("zsh", Some(Kind::Zsh)),
        ("/bin/bash", Some(Kind::Bash)),
        ("/x/sh", Some(Kind::Sh)),
        ("bash.exe", Some(Kind::Bash)),
        ("/x/bash.old.exe", Some(Kind::Bash)),
        ("pwsh", Some(Kind::PowerShell)),
        ("/x/powershell", Some(Kind::PowerShell)),
        ("cmd", Some(Kind::Cmd)),
        ("/usr/bin/fish", None),
        ("/bin/zsh-5.9", None),
        ("", None),
    ] {
        assert_eq!(kind_of(Path::new(path)), kind, "{path}");
    }
    // `argv` is `argv_on` with this machine's login shell and file system: a program that is
    // there, with Codex's arguments.
    let (program, args) = argv(line, &env);
    assert!(program.is_file(), "{program:?}");
    assert_eq!(args.last(), Some(&OsString::from(line)));
}

/// The limit is 30 s on Windows, for a cold PowerShell start, and 5 s elsewhere.
#[test]
fn the_limit_is_30_s_on_windows_and_5_s_elsewhere() {
    assert_eq!(limit_for(true), Duration::from_secs(30));
    assert_eq!(limit_for(false), Duration::from_secs(5));
    assert_eq!(LIMIT, limit_for(cfg!(windows)));
}

/// The command is the hook's own for this OS; a hooks object without one fails as `NoEntry`.
#[test]
fn the_self_test_runs_the_hooks_command_for_this_os() {
    let entry = entry_for(Path::new(&crate::testing::abs("/opt/g")));
    let key = if cfg!(windows) {
        "commandWindows"
    } else {
        "command"
    };
    assert_eq!(
        command_of(&entry).unwrap(),
        entry["PreToolUse"][0]["hooks"][0][key].as_str().unwrap()
    );
    let empty = Map::new();
    assert_eq!(command_of(&empty), None);
    let scratch = Scratch::new("no-entry");
    assert_eq!(scratch.run(&empty, LIMIT), Err(Failed::NoEntry));
    assert_eq!(
        run(None, b"", &scratch.0, &Scratch::env()),
        Err(Failed::NoEntry)
    );
}

#[test]
fn a_missing_guard_fails_the_self_test() {
    let scratch = Scratch::new("missing");
    let entry = entry_for(&scratch.0.join("no such guard"));
    assert_eq!(scratch.run(&entry, LIMIT), Err(Failed::Answer));
}

#[cfg(unix)]
#[test]
fn a_guard_that_lets_the_call_go_fails_the_self_test() {
    let scratch = Scratch::new("exit-0");
    for body in [
        "cat > /dev/null; exit 0",
        // Exit 2 with nothing on stderr is no refusal.
        "cat > /dev/null; exit 2",
        // Nor is something on stderr with another code.
        "echo no >&2; exit 1",
        // Exit 2 with the Windows line's own ending for a guard that did not answer.
        "cat > /dev/null; echo 'Clauseo could not check this step, so it did not run.' >&2; exit 2",
    ] {
        let entry = entry_for(&scratch.guard(body));
        assert_eq!(scratch.run(&entry, LIMIT), Err(Failed::Answer), "{body}");
    }
}

#[cfg(unix)]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the self-test's limit is in real time"
)]
fn a_hung_guard_fails_the_self_test_at_the_limit() {
    let scratch = Scratch::new("hung");
    let entry = entry_for(&scratch.guard("exec sleep 60"));
    let start = Instant::now();
    assert_eq!(
        scratch.run(&entry, Duration::from_millis(500)),
        Err(Failed::Late)
    );
    let took = start.elapsed();
    assert!(took >= Duration::from_millis(500), "{took:?}");
    assert!(took < Duration::from_secs(10), "{took:?}");
}

/// The first run passes on a refusal.
#[cfg(unix)]
#[test]
fn a_guard_that_refuses_passes_the_first_run() {
    let scratch = Scratch::new("refuses");
    let entry = entry_for(&scratch.guard("cat > /dev/null; echo refused >&2; exit 2"));
    assert_eq!(scratch.run(&entry, LIMIT), Ok(()));
}

/// The body of a guard script that refuses the self-test's unknown session and answers any other
/// call with `then`.
#[cfg(unix)]
fn answering(then: &str) -> String {
    format!(
        "input=$(cat)\ncase \"$input\" in *'\"session_id\":\"clauseo-self-test\"'*) echo refused >&2; \
         exit 2;; esac\n{then}"
    )
}

/// A guard that refuses every call, the conversation's own harmless one too, fails the
/// self-test at its second run.
#[cfg(unix)]
#[test]
fn a_guard_that_refuses_every_call_fails_the_self_test() {
    let scratch = Scratch::new("refuses-all");
    let entry = entry_for(&scratch.guard("cat > /dev/null; echo refused >&2; exit 2"));
    assert_eq!(scratch.test(entry).run("main"), Err(Failed::Refused));
}

/// The second run passes on exit 0 with nothing or a JSON value on stdout, and fails on any
/// other answer.
#[cfg(unix)]
#[test]
fn the_second_run_needs_exit_0_and_empty_or_json_stdout() {
    let scratch = Scratch::new("second-run");
    for (then, expected) in [
        ("exit 0", Ok(())),
        ("echo '{}'; exit 0", Ok(())),
        ("echo; exit 0", Ok(())),
        ("echo 'not json'; exit 0", Err(Failed::Refused)),
        ("exit 1", Err(Failed::Refused)),
        ("echo no >&2; exit 2", Err(Failed::Refused)),
    ] {
        let entry = entry_for(&scratch.guard(&answering(then)));
        assert_eq!(scratch.test(entry).run("main"), expected, "{then}");
    }
}

/// On Windows the self-test runs the hook's `commandWindows` as written: #1186's one PowerShell
/// line (`$global:LASTEXITCODE = -1; & '<program>' …; exit 2`) under PowerShell. A guard that
/// refuses passes; one that exits 0 fails; a missing one fails too, though the line
/// ends with 2, since its stderr is the line's own [`crate::pre_write_guard::UNCHECKED`].
#[cfg(windows)]
#[test]
fn the_self_test_runs_the_windows_powershell_line_as_written() {
    let scratch = Scratch::new("ps-line");
    let guard = |name: &str, code: u8| {
        let path = scratch.0.join(name);
        let body = format!("@echo off\r\nmore > nul\r\necho refused 1>&2\r\nexit /b {code}\r\n");
        write::write_file(&path, body.as_bytes()).unwrap();
        path
    };
    let refuses = guard("guard refuses.cmd", 2);
    let entry = entry_for(&refuses);
    let line = command_of(&entry).unwrap();
    let quoted = format!("'{}'", refuses.display().to_string().replace('\'', "''"));
    assert!(
        line.starts_with(&format!(
            "$global:LASTEXITCODE = -1; & {quoted} --pre-write-guard; $c = $LASTEXITCODE; "
        )),
        "{line}"
    );
    assert!(line.ends_with("; exit 2"), "{line}");
    assert_eq!(scratch.run(&entry, LIMIT), Ok(()), "{line}");
    let lets_go = entry_for(&guard("guard lets go.cmd", 0));
    assert_eq!(scratch.run(&lets_go, LIMIT), Err(Failed::Answer));
    let missing = entry_for(&scratch.0.join("no such guard.exe"));
    assert_eq!(scratch.run(&missing, LIMIT), Err(Failed::Answer));
}

/// The environment variable that makes a real-guard test below act as the guard, and the one
/// with its arguments.
#[cfg(unix)]
const CHILD: &str = "CLAUSEO_TEST_SELF_TEST_GUARD";
#[cfg(unix)]
const CHILD_ARGS: &str = "CLAUSEO_TEST_SELF_TEST_GUARD_ARGS";

/// In the test binary run again with [`CHILD`] set: the real guard, as the desktop app runs it
/// (`arm`, then `run` on stdin and stderr), on the arguments in [`CHILD_ARGS`], exiting with its
/// code. Nothing otherwise.
#[cfg(unix)]
#[expect(
    clippy::exit,
    reason = "the test binary, run again as the guard, exits with its code"
)]
fn act_as_guard() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let args: Vec<OsString> = std::env::var(CHILD_ARGS)
        .unwrap()
        .split('\n')
        // The script, then the guard's flag.
        .skip(2)
        .map(OsString::from)
        .collect();
    crate::pre_write_guard::sessions::arm(&args);
    let code = crate::pre_write_guard::run(
        &args,
        std::io::stdin().lock(),
        std::io::sink(),
        std::io::stderr(),
    );
    std::process::exit(code);
}

/// The real guard behind the hook command with a record and an ids file in `scratch`, the test
/// binary's test `test` run again as the guard ([`act_as_guard`]): its self-test and the ids file.
#[cfg(unix)]
fn real_guard(scratch: &Scratch, test: &str) -> (SelfTest, PathBuf) {
    let data = scratch.0.join("data");
    write::create_dir_all(&data).unwrap();
    let (record, id) = crate::pre_write_guard::make_record(&data, "s1").unwrap();
    let exe = std::env::current_exe().unwrap();
    let body = format!(
        "args=$(printf '%s\\n' \"$0\" \"$@\")\nexec env {CHILD}=1 {CHILD_ARGS}=\"$args\" {} \
         --exact pre_write_guard::self_test::tests::{test} --nocapture --test-threads=1 > /dev/null",
        posix_quoted(exe.to_str().unwrap())
    );
    let program = scratch.guard(&body);
    let config =
        crate::agent::loads::codex_hook_config(&program, Some((&record, id.as_str())), &[])
            .unwrap();
    let entry = config.as_object().unwrap().clone();
    assert!(command_of(&entry).unwrap().contains("--sessions"));
    let ids = crate::pre_write_guard::sessions::file(record.parent().unwrap());
    (scratch.test(entry), ids)
}

/// The real guard with the conversation's session listed passes both runs: it refuses the
/// unknown session's call and allows the conversation's own, a harmless call and a shell
/// command alike.
#[cfg(unix)]
#[test]
fn the_real_guard_passes_the_self_test() {
    act_as_guard();
    let scratch = Scratch::new("real");
    let (test, ids) = real_guard(&scratch, "the_real_guard_passes_the_self_test");
    crate::pre_write_guard::sessions::add(ids.parent().unwrap(), "main-session").unwrap();
    assert_eq!(test.run("main-session"), Ok(()));
    let mut known: Value = serde_json::from_slice(&input(&scratch.0)).unwrap();
    known["session_id"] = json!("main-session");
    for call in [
        known.to_string().into_bytes(),
        own_input(&scratch.0, "main-session"),
    ] {
        let allowed = allows_within(Some(&test.entry), (&call, &scratch.0, &test.env), LIMIT);
        assert_eq!(allowed, Ok(()));
    }
    // Another session is refused at the second run.
    assert_eq!(test.run("other-session"), Err(Failed::Refused));
}

/// The real guard whose ids file is missing, not readable as one, or a folder fails the
/// self-test at its second run: it would refuse every call of the session.
#[cfg(unix)]
#[test]
fn a_wrong_or_unreadable_ids_file_fails_the_self_test() {
    act_as_guard();
    let scratch = Scratch::new("bad-ids");
    let (test, ids) = real_guard(
        &scratch,
        "a_wrong_or_unreadable_ids_file_fails_the_self_test",
    );
    assert!(!ids.exists());
    assert_eq!(test.run("main-session"), Err(Failed::Refused));
    write::write_file(&ids, b"not json").unwrap();
    assert_eq!(test.run("main-session"), Err(Failed::Refused));
    write::remove_file(&ids).unwrap();
    write::create_dir_all(&ids).unwrap();
    assert_eq!(test.run("main-session"), Err(Failed::Refused));
}
