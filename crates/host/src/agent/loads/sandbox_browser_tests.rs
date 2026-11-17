//! What Claude's sandbox keeps from the guarded browser: loopback in every spelling Claude Code's
//! proxy may see, and agent-browser's socket folders.

use serde_json::json;

use super::sandbox::{ClaudeSandbox, SandboxOs};
use super::sandbox_tests::{Session, mac};

/// Loopback and the any-address are denied in every spelling the proxy may see: names, IPv4,
/// IPv6, and IPv4-mapped IPv6 as the URL parser writes it (`[::ffff:7f00:1]`, `[::ffff:0:0]`)
/// and as typed, each with and without brackets.
#[test]
fn every_loopback_spelling_is_denied() {
    assert_eq!(
        super::sandbox::DENIED_DOMAINS,
        [
            "localhost",
            "127.0.0.1",
            "::1",
            "[::1]",
            "0.0.0.0",
            "[::]",
            "::ffff:7f00:1",
            "[::ffff:7f00:1]",
            "::ffff:0:0",
            "[::ffff:0:0]",
            "::ffff:127.0.0.1",
            "[::ffff:127.0.0.1]",
            "::ffff:0.0.0.0",
            "[::ffff:0.0.0.0]",
        ]
    );
    let sandbox = mac(&Session::new("s-1").paths()).unwrap();
    assert_eq!(
        sandbox.settings()["network"]["deniedDomains"],
        json!(super::sandbox::DENIED_DOMAINS)
    );
}

/// agent-browser's socket folders, every session's, are denied for reading and, where the write
/// lists read globs (macOS), for writing; the globs never turn the sandbox off as a `*` in a
/// session path does.
#[test]
fn the_browser_socket_folders_are_denied() {
    let session = Session::new("s-1");
    let on_mac = mac(&session.paths()).unwrap();
    for dir in super::sandbox::SOCKET_DIRS {
        assert!(on_mac.deny_read.iter().any(|p| p == dir), "{dir}");
        assert!(on_mac.deny_write.iter().any(|p| p == dir), "{dir}");
    }
    let settings = on_mac.settings();
    let filesystem = &settings["filesystem"];
    for key in ["denyRead", "denyWrite"] {
        let list = filesystem[key].as_array().unwrap();
        assert!(list.iter().any(|p| p == "/tmp/clauseo-ab-*"), "{key}");
    }
    let linux = ClaudeSandbox::for_os(SandboxOs::Linux, &session.paths()).unwrap();
    assert!(linux.deny_read.iter().any(|p| p == "/tmp/clauseo-ab-*"));
    assert!(!linux.deny_read.iter().any(|p| p.starts_with("/private/")));
    assert!(!linux.deny_write.iter().any(|p| p.contains("clauseo-ab")));
}
