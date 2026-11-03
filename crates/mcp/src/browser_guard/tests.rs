use std::ffi::OsString;

use super::env::Os;
use super::{Command, parse};

/// An existing file and folder at absolute paths: this test binary and its folder.
fn program_and_cwd() -> (String, String) {
    let exe = std::env::current_exe().unwrap();
    let cwd = exe.parent().unwrap().to_str().unwrap().to_owned();
    (exe.to_str().unwrap().to_owned(), cwd)
}

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

#[test]
fn the_cdp_port_is_required_and_parsed() {
    let (program, cwd) = program_and_cwd();
    let parsed = parse(
        &args(&[
            "--agent-browser",
            &program,
            "--cwd",
            &cwd,
            "--cdp-port",
            "61101",
            "--",
            "mcp",
        ]),
        Os::Unix,
    )
    .unwrap();
    assert_eq!(
        parsed,
        Command {
            agent_browser: program.clone().into(),
            cwd: cwd.clone().into(),
            cdp_port: 61_101,
            daemon_port: None,
            child_args: args(&["mcp"]),
        }
    );
    let without = parse(
        &args(&["--agent-browser", &program, "--cwd", &cwd, "--", "mcp"]),
        Os::Unix,
    );
    assert!(without.unwrap_err().starts_with("expected --agent-browser"));
}

#[test]
fn a_bad_cdp_port_is_refused() {
    let (program, cwd) = program_and_cwd();
    for bad in [
        "", "0", "80", "1023", "65536", "070000", "01024", "+1024", "1024 ", "x", "-1",
    ] {
        let error = parse(
            &args(&[
                "--agent-browser",
                &program,
                "--cwd",
                &cwd,
                "--cdp-port",
                bad,
                "--",
            ]),
            Os::Unix,
        )
        .unwrap_err();
        assert_eq!(
            error, "--cdp-port must be a port from 1024 to 65535 with no leading zero",
            "{bad:?}"
        );
    }
    for good in ["1024", "65535"] {
        let parsed = parse(
            &args(&[
                "--agent-browser",
                &program,
                "--cwd",
                &cwd,
                "--cdp-port",
                good,
                "--",
            ]),
            Os::Unix,
        );
        assert!(parsed.is_ok(), "{good}");
    }
}

#[test]
fn the_daemon_port_is_windows_only_and_required_there() {
    let (program, cwd) = program_and_cwd();
    let with_daemon = args(&[
        "--agent-browser",
        &program,
        "--cwd",
        &cwd,
        "--cdp-port",
        "61101",
        "--daemon-port",
        "61102",
        "--",
        "mcp",
    ]);
    assert_eq!(
        parse(&with_daemon, Os::Unix).unwrap_err(),
        "--daemon-port is for Windows only"
    );
    let parsed = parse(&with_daemon, Os::Windows).unwrap();
    assert_eq!(parsed.daemon_port, Some(61_102));
    assert_eq!(parsed.child_args, args(&["mcp"]));

    let without = args(&[
        "--agent-browser",
        &program,
        "--cwd",
        &cwd,
        "--cdp-port",
        "61101",
        "--",
        "mcp",
    ]);
    assert!(parse(&without, Os::Windows).unwrap_err().starts_with(
        "expected --agent-browser <file> --cwd <folder> --cdp-port <port> --daemon-port <port>"
    ));
    let bad = args(&[
        "--agent-browser",
        &program,
        "--cwd",
        &cwd,
        "--cdp-port",
        "61101",
        "--daemon-port",
        "80",
        "--",
    ]);
    assert_eq!(
        parse(&bad, Os::Windows).unwrap_err(),
        "--daemon-port must be a port from 1024 to 65535 with no leading zero"
    );
}
