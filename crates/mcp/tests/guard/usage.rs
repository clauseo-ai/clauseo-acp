//! The command line: the mode table's usage line, and the guard's own refusals before it starts
//! anything.

use std::ffi::OsStr;
use std::process::Command;

use crate::support::{GUARD, Scratch, example, guard_command, run};
use crate::write;

const MODE_USAGE: &str = "usage: clauseo-mcp <mode> ... (modes: browser-guard)\n";
const GUARD_USAGE: &str = "usage: clauseo-mcp browser-guard --agent-browser <file> --cwd <folder> \
                           -- <agent-browser args...>\n";

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
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec![], "expected --agent-browser"),
        (
            vec!["--agent-browser", stub, "--cwd", cwd],
            "expected --agent-browser",
        ),
        (
            vec!["--agent-browser", stub, "--cwd", cwd, log],
            "expected --agent-browser",
        ),
        (
            vec!["--cwd", cwd, "--agent-browser", stub, "--", log],
            "expected --agent-browser",
        ),
        (
            vec!["--agent-browser=x", stub, "--cwd", cwd, "--", log],
            "expected --agent-browser",
        ),
        (
            vec!["--agent-browser", "echo_stub", "--cwd", cwd, "--", log],
            "--agent-browser must",
        ),
        (
            vec!["--agent-browser", missing, "--cwd", cwd, "--", log],
            "--agent-browser must",
        ),
        (
            vec!["--agent-browser", cwd, "--cwd", cwd, "--", log],
            "--agent-browser must",
        ),
        (
            vec!["--agent-browser", stub, "--cwd", "state", "--", log],
            "--cwd must",
        ),
        (
            vec!["--agent-browser", stub, "--cwd", missing, "--", log],
            "--cwd must",
        ),
        (
            vec!["--agent-browser", stub, "--cwd", file, "--", log],
            "--cwd must",
        ),
    ];
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
    for name in [
        "AGENT_BROWSER_CONFIG",
        "AGENT_BROWSER_PROXY",
        "AGENT_BROWSER_EXECUTABLE_PATH",
        "AGENT_BROWSER_SESSION",
    ] {
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
        !scratch.cwd().join("run").exists() && !scratch.socket_dir().exists(),
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
    let output = run(guard_command(&scratch, &not_a_program, &args));
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).starts_with("clauseo-mcp browser-guard: cannot run "));
}

/// Unix: a socket folder already in `/tmp` that is a link, or not mode 0700, stops the guard
/// with exit 2 before anything starts.
#[cfg(unix)]
#[test]
fn an_unsafe_socket_folder_stops_the_guard() {
    let scratch = Scratch::new("bad_socket_dir");
    let log = scratch.log();
    let check = |words: &str| {
        let output = run(guard_command(
            &scratch,
            &example("echo_stub"),
            &[log.as_os_str()],
        ));
        assert_eq!(output.status.code(), Some(2), "{words}");
        let text = stderr(&output);
        assert!(
            text.starts_with("clauseo-mcp browser-guard: the socket folder /tmp/clauseo-ab-"),
            "{text}"
        );
        assert!(text.contains(words), "{text}");
        assert!(!scratch.log().exists(), "the stub never started");
    };
    write::create_dir_all(scratch.socket_dir()).unwrap();
    write::set_mode(scratch.socket_dir(), 0o755).unwrap();
    check("its mode is 755, not 700");
    write::remove_dir_all(scratch.socket_dir()).unwrap();
    write::symlink(scratch.cwd(), scratch.socket_dir()).unwrap();
    check("it is a link");
}
