//! The bridge's command line on each OS, checked on every OS: `--socket` on Unix, `--pipe` and
//! `--server-sid` on Windows, and each refusing the other's options with a plain reason.

use std::ffi::OsString;
use std::path::PathBuf;

use super::{Target, parse};
use crate::transport::{Identity, Peer};

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

const SID: &str = "S-1-5-21-3623811015-3361044348-30300820-1013";

/// The socket and the guard's identity on Unix (the numbers' own checks are in `tests.rs`).
#[test]
fn unix_takes_an_absolute_socket_and_the_guards_identity_only() {
    let identity = ["--guard-pid", "4242", "--guard-pidversion", "7"];
    let with = |first: &[&str]| args(&[first, &identity[..]].concat());
    assert_eq!(
        parse(&with(&["--socket", "/tmp/x/g.sock"]), false),
        Ok(Target::Socket {
            socket: PathBuf::from("/tmp/x/g.sock"),
            guard: Peer::Process(Identity {
                pid: 4242,
                version: 7
            }),
        })
    );
    assert_eq!(
        parse(&with(&["--socket", "g.sock"]), false),
        Err("--socket must be an absolute path")
    );
    assert_eq!(
        parse(
            &args(&["--pipe", "clauseo-ab-1", "--server-sid", SID]),
            false
        ),
        Err("--pipe and --server-sid are for Windows only")
    );
    assert_eq!(
        parse(&args(&["--server-sid", SID]), false),
        Err("--pipe and --server-sid are for Windows only")
    );
    let expected = Err("expected --socket <path> --guard-pid <pid> --guard-pidversion <version>");
    assert_eq!(parse(&args(&[]), false), expected);
    assert_eq!(parse(&args(&["--socket", "/a"]), false), expected);
}

/// Windows takes no guard identity: the pipe's owner is its check.
#[test]
fn windows_refuses_the_unix_identity_options() {
    assert_eq!(
        parse(
            &args(&["--pipe", "p", "--server-sid", SID, "--guard-pid", "4242"]),
            true
        ),
        Err("expected --pipe <name> --server-sid <sid>")
    );
    assert_eq!(
        parse(&args(&["--guard-pid", "4242"]), true),
        Err("expected --pipe <name> --server-sid <sid>")
    );
}

#[test]
fn windows_takes_a_pipe_name_and_the_server_sid() {
    assert_eq!(
        parse(
            &args(&["--pipe", "clauseo-ab-1", "--server-sid", SID]),
            true
        ),
        Ok(Target::Pipe {
            name: "clauseo-ab-1".to_owned(),
            server_sid: SID.to_owned(),
        })
    );
    assert_eq!(
        parse(&args(&["--socket", "C:\\g.sock"]), true),
        Err("--socket is for macOS and Linux only")
    );
    for name in ["a\\b", "..", "a b", "", "a.b"] {
        assert_eq!(
            parse(&args(&["--pipe", name, "--server-sid", SID]), true),
            Err("--pipe must be 1 to 200 letters, digits and dashes"),
            "{name:?}"
        );
    }
    for sid in ["WD", "S-1-5", "S-1-5-18)(A;;GA;;;WD"] {
        assert_eq!(
            parse(&args(&["--pipe", "p", "--server-sid", sid]), true),
            Err("--server-sid must be a SID string, S-1-..."),
            "{sid:?}"
        );
    }
    for line in [
        &["--pipe", "p"][..],
        &["--server-sid", SID, "--pipe", "p"],
        &["--pipe", "p", "--server-sid", SID, "extra"],
    ] {
        assert_eq!(
            parse(&args(line), true),
            Err("expected --pipe <name> --server-sid <sid>"),
            "{line:?}"
        );
    }
}
