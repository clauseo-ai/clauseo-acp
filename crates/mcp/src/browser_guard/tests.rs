use std::ffi::OsString;

use super::env::Os;
use super::{Command, Listen, parse};

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
            listen: None,
            child_args: args(&["mcp"]),
        }
    );
    let without = parse(
        &args(&["--agent-browser", &program, "--cwd", &cwd, "--", "mcp"]),
        Os::Unix,
    );
    assert!(without.unwrap_err().starts_with("expected --agent-browser"));
}

/// `--listen <socket>` may come first on Unix, as an absolute path that need not exist; it is
/// refused when relative, anywhere else in the line, and on Windows. Unix hosts only: on
/// Windows `/tmp/...` is no absolute path (it has no drive).
#[cfg(unix)]
#[test]
fn listen_comes_first_on_unix_only() {
    let (program, cwd) = program_and_cwd();
    let rest = [
        "--agent-browser",
        program.as_str(),
        "--cwd",
        cwd.as_str(),
        "--cdp-port",
        "61101",
        "--",
        "mcp",
    ];
    let with = |first: &[&str]| args(&[first, &rest[..]].concat());
    let parsed = parse(
        &with(&["--listen", "/tmp/clauseo-ab-x/guard.sock"]),
        Os::Unix,
    )
    .unwrap();
    assert_eq!(
        parsed.listen,
        Some(Listen::Socket("/tmp/clauseo-ab-x/guard.sock".into()))
    );
    assert_eq!(parsed.child_args, args(&["mcp"]));
    assert_eq!(
        parse(&with(&["--listen", "guard.sock"]), Os::Unix),
        Err("--listen must be an absolute path")
    );
    assert_eq!(
        parse(&with(&["--listen", "C:\\guard.sock"]), Os::Windows),
        Err("--listen is for macOS and Linux only")
    );
    assert!(parse(&with(&["--listen"]), Os::Unix).is_err());
    let late = args(&[&rest[..6], &["--listen", "/tmp/g.sock"], &rest[6..]].concat());
    assert!(parse(&late, Os::Unix).is_err(), "--listen after the ports");
    assert_eq!(
        parse(
            &with(&["--listen-pipe", "p", "--client-sid", "S-1-5-18"]),
            Os::Unix
        ),
        Err("--listen-pipe, --client-sid and --client-package-sid are for Windows only")
    );
    assert_eq!(
        parse(&with(&["--client-sid", "S-1-5-18"]), Os::Unix),
        Err("--listen-pipe, --client-sid and --client-package-sid are for Windows only")
    );
}

/// `--listen-pipe <name> --client-sid <SID>` may come first on Windows, both checked; it is
/// refused in any other order, with a bad name or SID, anywhere else in the line, and on Unix.
#[test]
fn listen_pipe_comes_first_on_windows_only() {
    let (program, cwd) = program_and_cwd();
    let rest = [
        "--agent-browser",
        program.as_str(),
        "--cwd",
        cwd.as_str(),
        "--cdp-port",
        "61101",
        "--daemon-port",
        "61102",
        "--",
        "mcp",
    ];
    let with = |first: &[&str]| args(&[first, &rest[..]].concat());
    let sid = "S-1-5-21-3623811015-3361044348-30300820-1013";
    let parsed = parse(
        &with(&["--listen-pipe", "clauseo-ab-0f3a", "--client-sid", sid]),
        Os::Windows,
    )
    .unwrap();
    assert_eq!(
        parsed.listen,
        Some(Listen::Pipe {
            name: "clauseo-ab-0f3a".to_owned(),
            client_sid: sid.to_owned(),
            package_sid: None,
        })
    );
    assert_eq!(parsed.daemon_port, Some(61_102));
    assert_eq!(parsed.child_args, args(&["mcp"]));
    for name in ["a\\b", "..\\x", "a b", "", "a.b", "a_b"] {
        assert_eq!(
            parse(
                &with(&["--listen-pipe", name, "--client-sid", sid]),
                Os::Windows
            ),
            Err("--listen-pipe must be 1 to 200 letters, digits and dashes"),
            "{name:?}"
        );
    }
    for bad in ["WD", "S-1-5", "S-1-5-18)(A;;GA;;;WD", ""] {
        assert_eq!(
            parse(
                &with(&["--listen-pipe", "p", "--client-sid", bad]),
                Os::Windows
            ),
            Err("--client-sid must be a SID string, S-1-..."),
            "{bad:?}"
        );
    }
    let expected = Err("expected --listen-pipe <name> --client-sid <sid> first");
    assert_eq!(
        parse(
            &with(&["--client-sid", sid, "--listen-pipe", "p"]),
            Os::Windows
        ),
        expected
    );
    assert_eq!(parse(&with(&["--listen-pipe", "p"]), Os::Windows), expected);
    let late = args(
        &[
            &rest[..8],
            &["--listen-pipe", "p", "--client-sid", sid],
            &rest[8..],
        ]
        .concat(),
    );
    assert!(
        parse(&late, Os::Windows).is_err(),
        "--listen-pipe after the ports"
    );
}

/// `--client-package-sid <SID>` may follow `--client-sid` on Windows; it must be an
/// `AppContainer` package SID (`S-1-15-2-` and seven numbers), never ALL APPLICATION PACKAGES.
#[test]
fn the_client_package_sid_is_optional_and_checked() {
    let (program, cwd) = program_and_cwd();
    let rest = [
        "--agent-browser",
        program.as_str(),
        "--cwd",
        cwd.as_str(),
        "--cdp-port",
        "61101",
        "--daemon-port",
        "61102",
        "--",
        "mcp",
    ];
    let with = |first: &[&str]| args(&[first, &rest[..]].concat());
    let sid = "S-1-5-21-3623811015-3361044348-30300820-1013";
    let package = "S-1-15-2-2434737943-167758768-3180539153-984336765-1107280622-3591121930-26772";
    let parsed = parse(
        &with(&[
            "--listen-pipe",
            "p",
            "--client-sid",
            sid,
            "--client-package-sid",
            package,
        ]),
        Os::Windows,
    )
    .unwrap();
    assert_eq!(
        parsed.listen,
        Some(Listen::Pipe {
            name: "p".to_owned(),
            client_sid: sid.to_owned(),
            package_sid: Some(package.to_owned()),
        })
    );
    assert_eq!(parsed.child_args, args(&["mcp"]));
    for bad in [
        "S-1-5-18",
        "S-1-15-2-1",
        "S-1-15-3-1024-1-2-3-4-5-6-7-8",
        "WD",
        "",
    ] {
        assert_eq!(
            parse(
                &with(&[
                    "--listen-pipe",
                    "p",
                    "--client-sid",
                    sid,
                    "--client-package-sid",
                    bad
                ]),
                Os::Windows
            ),
            Err(
                "--client-package-sid must be an AppContainer package SID, S-1-15-2- and seven numbers"
            ),
            "{bad:?}"
        );
    }
    assert_eq!(
        parse(
            &with(&[
                "--listen-pipe",
                "p",
                "--client-sid",
                sid,
                "--client-package-sid"
            ]),
            Os::Windows
        )
        .map(|_| ()),
        Err(
            "--client-package-sid must be an AppContainer package SID, S-1-15-2- and seven numbers"
        ),
        "a package flag with no SID takes the next argument as its SID"
    );
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
