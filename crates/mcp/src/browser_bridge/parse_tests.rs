//! The bridge's command line on each OS, checked on every OS: `--socket` on Unix, `--pipe` and
//! `--server-sid` on Windows, then on both the guard's identity, and each OS refusing the other's
//! options with a plain reason.

use std::ffi::OsString;
use std::path::PathBuf;

use super::{Target, parse};
use crate::transport::{Identity, Peer};

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

const SID: &str = "S-1-5-21-3623811015-3361044348-30300820-1013";
const IDENTITY: [&str; 4] = ["--guard-pid", "4242", "--guard-pidversion", "7"];
const GUARD: Peer = Peer::Process(Identity {
    pid: 4242,
    version: 7,
});
const EXPECTED_PIPE: Result<Target, &str> =
    Err("expected --pipe <name> --server-sid <sid> --guard-pid <pid> --guard-pidversion <version>");

/// `first`, then the guard's identity.
fn with(first: &[&str]) -> Vec<OsString> {
    args(&[first, &IDENTITY[..]].concat())
}

/// The socket and the guard's identity on Unix (the numbers' own checks are in `tests.rs`).
#[test]
fn unix_takes_an_absolute_socket_and_the_guards_identity_only() {
    assert_eq!(
        parse(&with(&["--socket", "/tmp/x/g.sock"]), false),
        Ok(Target::Socket {
            socket: PathBuf::from("/tmp/x/g.sock"),
            guard: GUARD,
        })
    );
    assert_eq!(
        parse(&with(&["--socket", "g.sock"]), false),
        Err("--socket must be an absolute path")
    );
    for line in [
        &["--pipe", "clauseo-ab-1", "--server-sid", SID][..],
        &["--server-sid", SID],
    ] {
        assert_eq!(
            parse(&with(line), false),
            Err("--pipe and --server-sid are for Windows only")
        );
    }
    let expected = Err("expected --socket <path> --guard-pid <pid> --guard-pidversion <version>");
    assert_eq!(parse(&args(&[]), false), expected);
    assert_eq!(parse(&args(&["--socket", "/a"]), false), expected);
}

/// The pipe, the server SID and the guard's identity on Windows, all required.
#[test]
fn windows_takes_a_pipe_name_the_server_sid_and_the_guards_identity() {
    assert_eq!(
        parse(
            &with(&["--pipe", "clauseo-ab-1", "--server-sid", SID]),
            true
        ),
        Ok(Target::Pipe {
            name: "clauseo-ab-1".to_owned(),
            server_sid: SID.to_owned(),
            guard: GUARD,
        })
    );
    assert_eq!(
        parse(&with(&["--socket", "C:\\g.sock"]), true),
        Err("--socket is for macOS and Linux only")
    );
    for name in ["a\\b", "..", "a b", "", "a.b"] {
        assert_eq!(
            parse(&with(&["--pipe", name, "--server-sid", SID]), true),
            Err("--pipe must be 1 to 200 letters, digits and dashes"),
            "{name:?}"
        );
    }
    for sid in ["WD", "S-1-5", "S-1-5-18)(A;;GA;;;WD"] {
        assert_eq!(
            parse(&with(&["--pipe", "p", "--server-sid", sid]), true),
            Err("--server-sid must be a SID string, S-1-..."),
            "{sid:?}"
        );
    }
    let pipe = ["--pipe", "p", "--server-sid", SID];
    for (pid, version) in [
        ("0", "7"),
        ("1", "7"),
        ("04242", "7"),
        ("4242", "07"),
        ("x", "7"),
    ] {
        let line = args(
            &[
                &pipe[..],
                &["--guard-pid", pid, "--guard-pidversion", version],
            ]
            .concat(),
        );
        assert!(parse(&line, true).is_err(), "{pid} {version}");
    }
    for line in [
        &pipe[..],
        &["--pipe", "p"],
        &["--server-sid", SID, "--pipe", "p"],
        &["--guard-pid", "4242"],
    ] {
        assert_eq!(parse(&args(line), true), EXPECTED_PIPE, "{line:?}");
    }
    let extra = args(&[&pipe[..], &IDENTITY[..], &["extra"]].concat());
    assert_eq!(parse(&extra, true), EXPECTED_PIPE);
}
