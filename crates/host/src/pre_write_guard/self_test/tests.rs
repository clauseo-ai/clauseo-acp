//! The launch's self-test of Codex's guard hook, run as Codex runs it: the program and arguments
//! for each OS, from the configured hook line; a guard that is missing, lets the call go, says
//! nothing, or hangs past the limit fails it; one that refuses passes, and so does the real guard
//! (this test binary run again as the guard, on Unix).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

#[cfg(unix)]
use serde_json::json;
use serde_json::{Map, Value};

#[cfg(unix)]
use super::SelfTest;
use super::{Failed, LIMIT, argv, argv_on, command_of, input, limit_for, run, run_within};
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

    /// A plain environment, with `SHELL` set to `/bin/sh` on Unix.
    fn env() -> BTreeMap<OsString, OsString> {
        let mut env = BTreeMap::new();
        for name in ["PATH", "SystemRoot", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(name) {
                env.insert(OsString::from(name), value);
            }
        }
        #[cfg(unix)]
        env.insert(OsString::from("SHELL"), OsString::from("/bin/sh"));
        env
    }

    /// Runs the hook `entry` here, within `limit`.
    fn run(&self, entry: &Map<String, Value>, limit: Duration) -> Result<(), Failed> {
        run_within(Some(entry), (&input(&self.0), &self.0, &Self::env()), limit)
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
    let config = crate::agent::loads::codex_hook_config(program, None).unwrap();
    config.as_object().unwrap().clone()
}

/// `text` in POSIX single quotes.
#[cfg(unix)]
fn posix_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The argv for each OS is Codex's for the configured line itself: elsewhere the user's shell
/// with `-c <command>`; on Windows PowerShell with `-NoProfile -Command <commandWindows>`,
/// picked as Codex picks it, step by step (no file system: `exists` is given), and `cmd.exe /c`
/// with the line raw in quotes when there is none.
#[test]
fn the_argv_is_codexs_for_the_configured_line_on_each_os() {
    let config =
        crate::agent::loads::codex_hook_config(Path::new(&crate::testing::abs("/opt/g d/x")), None)
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
    let none = |_: &Path| false;
    assert_eq!(
        argv_on(
            posix,
            &env(&[("SHELL", OsString::from("/bin/zsh"))]),
            false,
            &none
        ),
        (
            PathBuf::from("/bin/zsh"),
            vec![OsString::from("-c"), OsString::from(posix)]
        )
    );
    assert_eq!(
        argv_on(posix, &env(&[]), false, &none).0,
        PathBuf::from("/bin/sh")
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
    let pick = |there: &[PathBuf]| {
        let exists = |path: &Path| there.iter().any(|file| file == path);
        argv_on(windows, &path_env, true, &exists)
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
    assert_eq!(
        argv_on(windows, &env(&[("PATH", store_path)]), true, &exists).0,
        store
    );
    // `argv` is `argv_on` with the real file system.
    assert_eq!(argv(posix, &env(&[]), false).0, PathBuf::from("/bin/sh"));
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
        // Exit 2 with nothing on stderr: Codex lets the call run.
        "cat > /dev/null; exit 2",
        // Something on stderr with another code: the same.
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

#[cfg(unix)]
#[test]
fn a_guard_that_refuses_passes_the_self_test() {
    let scratch = Scratch::new("refuses");
    let entry = entry_for(&scratch.guard("cat > /dev/null; echo refused >&2; exit 2"));
    assert_eq!(scratch.run(&entry, LIMIT), Ok(()));
}

/// On Windows the self-test runs the hook's `commandWindows` as written: #1186's one PowerShell
/// line (`$global:LASTEXITCODE = -1; & '<program>' …; exit 2`) under PowerShell. A guard that
/// refuses passes; one that lets the call go fails; a missing one fails too, though the line
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

/// The environment variable that makes [`the_real_guard_passes_the_self_test`] act as the guard,
/// and the one with its arguments.
#[cfg(unix)]
const CHILD: &str = "CLAUSEO_TEST_SELF_TEST_GUARD";
#[cfg(unix)]
const CHILD_ARGS: &str = "CLAUSEO_TEST_SELF_TEST_GUARD_ARGS";

/// The real guard, as the desktop app runs it (`arm`, then `run` on stdin and stderr), behind
/// the hook command with a record and an ids file: it refuses the self-test's call.
#[cfg(unix)]
#[test]
#[expect(
    clippy::exit,
    reason = "the test binary, run again as the guard, exits with its code"
)]
fn the_real_guard_passes_the_self_test() {
    if std::env::var_os(CHILD).is_some() {
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
    let scratch = Scratch::new("real");
    let data = scratch.0.join("data");
    write::create_dir_all(&data).unwrap();
    let (record, id) = crate::pre_write_guard::make_record(&data, "s1").unwrap();
    crate::pre_write_guard::sessions::add(record.parent().unwrap(), "main-session").unwrap();
    let exe = std::env::current_exe().unwrap();
    let body = format!(
        "args=$(printf '%s\\n' \"$0\" \"$@\")\nexec env {CHILD}=1 {CHILD_ARGS}=\"$args\" {} \
         --exact pre_write_guard::self_test::tests::the_real_guard_passes_the_self_test \
         --nocapture --test-threads=1 > /dev/null",
        posix_quoted(exe.to_str().unwrap())
    );
    let program = scratch.guard(&body);
    let config =
        crate::agent::loads::codex_hook_config(&program, Some((&record, id.as_str()))).unwrap();
    let entry = config.as_object().unwrap().clone();
    assert!(command_of(&entry).unwrap().contains("--sessions"));
    let test = SelfTest {
        entry,
        env: Scratch::env(),
        cwd: scratch.0.clone(),
    };
    assert_eq!(test.run(), Ok(()));
    // The same call from the conversation's own session passes the guard: it is the unknown
    // session it refused.
    let mut known: Value = serde_json::from_slice(&input(&scratch.0)).unwrap();
    known["session_id"] = json!("main-session");
    let known = run_within(
        Some(&test.entry),
        (known.to_string().as_bytes(), &scratch.0, &test.env),
        LIMIT,
    );
    assert_eq!(known, Err(Failed::Answer));
}
