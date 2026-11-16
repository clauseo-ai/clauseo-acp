//! The non-production connector addresses are compiled into their own builds only
//! (docs/connector-staging.md). The app binary is scanned for each:
//!
//! - the staging host, bare, which also covers `https://` plus the host and any path after it:
//!   none without `staging-connector`, which release.yml's binary scan relies on;
//! - the local test server's `127.0.0.1:8793`, which covers `http://` plus it: none without
//!   `local-connector`.
//!
//! With the feature, at least one copy, which shows the scan can see the string.
//!
//! No crate in the workspace turns on clauseo-connectors' `staging` or `local` outside those
//! features, not even in a dev-dependency, so the workspace's test build (whose features unify)
//! builds the binary without them too.

/// The binary under test.
const APP: &str = env!("CARGO_BIN_EXE_clauseo-desktop");

/// How many times `needle` appears in `haystack`, overlaps included.
fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

#[test]
fn the_staging_host_is_in_the_app_only_with_staging_connector() {
    // Built from pieces, so no copy of the host sits in this file as one literal.
    let host = ["data", "-staging", ".clauseo", ".chat"].concat();
    let app = std::fs::read(APP).unwrap();
    let found = count(&app, host.as_bytes());
    #[cfg(not(feature = "staging-connector"))]
    assert_eq!(
        found, 0,
        "{APP} holds the staging host without staging-connector"
    );
    #[cfg(feature = "staging-connector")]
    assert!(
        found >= 1,
        "{APP} lacks the staging host in a staging build"
    );
}

#[test]
fn the_local_server_is_in_the_app_only_with_local_connector() {
    // Built from pieces, as above.
    let address = ["127.0", ".0.1", ":87", "93"].concat();
    let app = std::fs::read(APP).unwrap();
    let found = count(&app, address.as_bytes());
    #[cfg(not(feature = "local-connector"))]
    assert_eq!(
        found, 0,
        "{APP} holds the local server's address without local-connector"
    );
    #[cfg(feature = "local-connector")]
    assert!(
        found >= 1,
        "{APP} lacks the local server's address in a local build"
    );
}

#[test]
fn the_count_finds_every_copy() {
    assert_eq!(count(b"xx-a.b-a.b", b"a.b"), 2);
    assert_eq!(count(b"", b"a.b"), 0);
}
