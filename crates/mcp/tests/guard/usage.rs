//! The command line: the mode table's usage line, and the guard's own refusals before it starts
//! anything.

use std::ffi::OsStr;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use crate::support::{CDP_PORT, GUARD, Guard, Scratch, example, guard_command, port_args, run};
use crate::write;

const MODE_USAGE: &str = if cfg!(unix) {
    "usage: clauseo-mcp <mode> ... (modes: browser-guard, browser-reaper, connector)\n"
} else {
    "usage: clauseo-mcp <mode> ... (modes: browser-guard, connector)\n"
};
const GUARD_USAGE: &str = if cfg!(windows) {
    "usage: clauseo-mcp browser-guard --agent-browser <file> --cwd <folder> --cdp-port <port> \
     --daemon-port <port> -- <agent-browser args...>\n"
} else {
    "usage: clauseo-mcp browser-guard --agent-browser <file> --cwd <folder> --cdp-port <port> \
     -- <agent-browser args...>\n"
};

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn no_mode_or_an_unknown_mode_prints_the_usage_line() {
    for args in [&[][..], &["nope"], &["Browser-Guard"], &["--help"]] {
        let mut command = Command::new(GUARD);
        command.args(args);
        let output = run(command);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(stderr(&output), MODE_USAGE, "{args:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    }
}

/// `--agent-browser <program> --cwd <folder>`, the ports, `--` and `log`.
fn with<'a>(program: &'a str, folder: &'a str, log: &'a str) -> Vec<&'a str> {
    let mut args = vec!["--agent-browser", program, "--cwd", folder];
    args.extend(port_args());
    args.extend(["--", log]);
    args
}

#[test]
fn a_bad_guard_command_line_exits_2_before_starting_anything() {
    let scratch = Scratch::new("bad_args");
    let stub = example("echo_stub");
    let stub = stub.to_str().unwrap();
    let cwd = scratch.cwd();
    let cwd = cwd.to_str().unwrap();
    let file = scratch.path().join("a-file");
    write::write(&file, "x").unwrap();
    let file = file.to_str().unwrap();
    let missing = scratch.path().join("missing");
    let missing = missing.to_str().unwrap();
    let log = scratch.log();
    let log = log.to_str().unwrap();
    let full = |program, folder| with(program, folder, log);
    let mut cases: Vec<(Vec<&str>, &str)> = vec![
        (vec![], "expected --agent-browser"),
        (
            vec!["--agent-browser", stub, "--cwd", cwd],
            "expected --agent-browser",
        ),
        (
            vec!["--agent-browser", stub, "--cwd", cwd, "--", log],
            "expected --agent-browser",
        ),
        (
            vec![
                "--agent-browser",
                stub,
                "--cwd",
                cwd,
                "--cdp-port",
                CDP_PORT,
                log,
            ],
            "expected --agent-browser",
        ),
        (
            vec![
                "--cwd",
                cwd,
                "--agent-browser",
                stub,
                "--cdp-port",
                CDP_PORT,
                "--",
                log,
            ],
            "expected --agent-browser",
        ),
        (
            vec![
                "--agent-browser=x",
                stub,
                "--cwd",
                cwd,
                "--cdp-port",
                CDP_PORT,
                "--",
                log,
            ],
            "expected --agent-browser",
        ),
        (full("echo_stub", cwd), "--agent-browser must"),
        (full(missing, cwd), "--agent-browser must"),
        (full(cwd, cwd), "--agent-browser must"),
        (full(stub, "state"), "--cwd must"),
        (full(stub, missing), "--cwd must"),
        (full(stub, file), "--cwd must"),
    ];
    for bad in ["80", "0", "65536", "08080", "x"] {
        let mut args = full(stub, cwd);
        args[5] = bad;
        cases.push((args, "--cdp-port must be a port from 1024 to 65535"));
    }
    if cfg!(unix) {
        cases.push((
            vec![
                "--agent-browser",
                stub,
                "--cwd",
                cwd,
                "--cdp-port",
                CDP_PORT,
                "--daemon-port",
                "61102",
                "--",
                log,
            ],
            "--daemon-port is for Windows only",
        ));
    } else {
        let mut args = full(stub, cwd);
        args[7] = "80";
        cases.push((args, "--daemon-port must be a port from 1024 to 65535"));
    }
    for (args, words) in cases {
        let mut command = Command::new(GUARD);
        command
            .arg("browser-guard")
            .args(&args)
            .envs(crate::support::host_env(&scratch));
        let output = run(command);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let text = stderr(&output);
        assert!(
            text.starts_with(&format!("clauseo-mcp browser-guard: {words}")),
            "{text}"
        );
        assert!(text.ends_with(GUARD_USAGE), "{text}");
        assert!(!scratch.log().exists(), "the stub never started: {args:?}");
    }
}

#[test]
fn a_missing_or_invalid_host_variable_is_named_and_nothing_starts() {
    let scratch = Scratch::new("bad_env");
    let stub = example("echo_stub");
    let log = scratch.log();
    let args = [log.as_os_str()];
    let mut required = vec![
        "AGENT_BROWSER_CONFIG",
        "AGENT_BROWSER_PROXY",
        "AGENT_BROWSER_EXECUTABLE_PATH",
        "AGENT_BROWSER_SESSION",
        "AGENT_BROWSER_STREAM_PORT",
    ];
    if cfg!(unix) {
        required.push("AGENT_BROWSER_SOCKET_DIR");
    }
    for name in required {
        let mut command = guard_command(&scratch, &stub, &args);
        command.env_remove(name);
        let output = run(command);
        assert_eq!(output.status.code(), Some(2), "{name}");
        assert_eq!(
            stderr(&output),
            format!("clauseo-mcp browser-guard: {name} is not set; the host sets it\n")
        );
    }
    for (name, value) in [
        ("AGENT_BROWSER_PROXY", "http://evil.example:1"),
        ("AGENT_BROWSER_PROXY", "http://127.0.0.1:0"),
        ("AGENT_BROWSER_CONFIG", "agent-browser.json"),
        ("AGENT_BROWSER_EXECUTABLE_PATH", "/no/such/chrome"),
        ("AGENT_BROWSER_SESSION", "a b"),
        ("AGENT_BROWSER_STREAM_PORT", "80"),
        ("AGENT_BROWSER_STREAM_PORT", "65536"),
        ("AGENT_BROWSER_STREAM_PORT", "0"),
    ] {
        let mut command = guard_command(&scratch, &stub, &args);
        command.env(name, value);
        let output = run(command);
        assert_eq!(output.status.code(), Some(2), "{name}={value}");
        assert!(
            stderr(&output).starts_with(&format!("clauseo-mcp browser-guard: {name} must")),
            "{}",
            stderr(&output)
        );
    }
    assert!(!scratch.log().exists(), "the stub never started");
    assert!(
        !scratch.cwd().join("run").exists() && !scratch.cwd().join("downloads").exists(),
        "no folder is made before the checks pass"
    );
}

#[test]
fn a_file_where_a_folder_goes_stops_the_guard() {
    let scratch = Scratch::new("bad_folder");
    write::write(scratch.cwd().join("tmp"), "x").unwrap();
    let log = scratch.log();
    let output = run(guard_command(
        &scratch,
        &example("echo_stub"),
        &[log.as_os_str()],
    ));
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(
        text.starts_with("clauseo-mcp browser-guard: cannot make the folder"),
        "{text}"
    );
    assert!(text.contains("it exists and is not a folder"), "{text}");
    assert!(!scratch.log().exists(), "the stub never started");
}

#[test]
fn an_unstartable_child_exits_1() {
    let scratch = Scratch::new("unstartable");
    let not_a_program = scratch.path().join("not-a-program");
    write::write(&not_a_program, "plain text").unwrap();
    let args: [&OsStr; 0] = [];
    // The first call starts Chrome, then fails to start agent-browser: the call gets an error.
    let mut guard = Guard::start(guard_command(&scratch, &not_a_program, &args));
    assert_eq!(guard.call("open")["error"]["code"], -32603);
    let (status, (_, text)) = guard.finish_with_stderr();
    assert_eq!(status.code(), Some(1));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.get(..2),
        Some(
            [
                "clauseo-mcp browser-guard: Chrome started; waiting for it to listen on the \
                 claimed debugging port 61101",
                "clauseo-mcp browser-guard: Chrome is listening on the claimed debugging port 61101"
            ]
            .as_slice()
        ),
        "{text}"
    );
    assert!(
        lines
            .get(2)
            .is_some_and(|line| line.starts_with("clauseo-mcp browser-guard: cannot run ")),
        "{text}"
    );
    // Chrome started first, so the guard ended it and deleted its profile.
    assert_eq!(
        crate::support::run_files(&scratch.cwd()),
        Vec::<PathBuf>::new()
    );
}

/// Unix: the host's socket folder must be there, absolute, a real folder (not a link), mode
/// 0700 and short enough for the socket path; otherwise the guard exits 2 before anything
/// starts, and makes no folder of its own.
#[cfg(unix)]
#[test]
fn an_unsafe_socket_folder_stops_the_guard() {
    let scratch = Scratch::new("bad_socket_dir");
    let log = scratch.log();
    let check = |dir: &Path, words: &str| {
        let mut command = guard_command(&scratch, &example("echo_stub"), &[log.as_os_str()]);
        command.env("AGENT_BROWSER_SOCKET_DIR", dir);
        let output = run(command);
        assert_eq!(output.status.code(), Some(2), "{words}");
        let text = stderr(&output);
        assert!(text.starts_with("clauseo-mcp browser-guard: "), "{text}");
        assert!(text.contains(words), "{words}: {text}");
        assert!(!scratch.log().exists(), "the stub never started");
        assert!(
            !scratch.cwd().join("downloads").exists(),
            "no folder is made before the checks pass"
        );
    };
    let dir = scratch.socket_dir();
    check(
        Path::new("tmp/clauseo-ab-relative"),
        "AGENT_BROWSER_SOCKET_DIR must be an absolute path",
    );
    write::remove_dir_all(&dir).unwrap();
    check(&dir, "cannot read it");
    assert!(
        std::fs::symlink_metadata(&dir).is_err(),
        "the guard makes no socket folder"
    );
    write::create_dir_all(&dir).unwrap();
    write::set_mode(&dir, 0o755).unwrap();
    check(&dir, "its mode is 755, not 700");
    write::remove_dir_all(&dir).unwrap();
    write::symlink(scratch.cwd(), &dir).unwrap();
    check(&dir, "it is a link");
    write::remove_dir_all(&dir).unwrap();
    // `<dir>/<session>.sock`, the session the run's own, at most 23 bytes
    // (`run-4294967295-ffffffff`): 104 bytes with a 75-byte folder.
    let long = std::path::PathBuf::from(format!("{}{}", dir.display(), "x".repeat(43)));
    assert_eq!(long.as_os_str().len(), 75);
    write::create_dir_all(&long).unwrap();
    write::set_mode(&long, 0o700).unwrap();
    check(&long, "is 104 bytes, over the 103 a Unix socket allows");
    write::remove_dir_all(&long).unwrap();
}

/// Linux: a socket folder another user owns (`/root`, root's own, mode 0700) stops the guard.
/// Skipped when the tests run as root, or `/root` is not mode 0700 on this machine.
#[cfg(target_os = "linux")]
#[test]
fn a_socket_folder_of_another_owner_stops_the_guard() {
    use std::os::unix::fs::MetadataExt;

    let Ok(root) = std::fs::symlink_metadata("/root") else {
        return;
    };
    let scratch = Scratch::new("other_owner");
    let my_uid = std::fs::metadata(scratch.cwd()).unwrap().uid();
    if my_uid == root.uid() || !root.is_dir() || root.mode() & 0o7777 != 0o700 {
        return;
    }
    let log = scratch.log();
    let mut command = guard_command(&scratch, &example("echo_stub"), &[log.as_os_str()]);
    command.env("AGENT_BROWSER_SOCKET_DIR", "/root");
    let output = run(command);
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(
        text.contains(&format!(
            "the socket folder /root: its owner is uid {}, not uid {my_uid}, the owner of --cwd",
            root.uid()
        )),
        "{text}"
    );
    assert!(!scratch.log().exists(), "the stub never started");
}
