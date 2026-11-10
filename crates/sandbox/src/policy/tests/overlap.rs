//! The Claude store and the browser socket folder kept off hidden paths: refused on, inside or
//! above a hidden matter; the store refused on or above a plain hidden path but built inside
//! one (Clauseo's login home is in the hidden data dir); the socket folder refused on, inside or
//! above any hidden path.

use std::path::{Path, PathBuf};

use super::{World, canon};
use crate::{Error, HiddenOverlapAllowance, Policy, PolicyBuilder};

const STORE: HiddenOverlapAllowance = HiddenOverlapAllowance::ClaudeStore;
const SOCKETS: HiddenOverlapAllowance = HiddenOverlapAllowance::BrowserSocketDir;

/// A world with the data dir and another matter, `m2`, holding `docs/`; `box/m3`, a matter in a
/// folder of its own; `box2/data2`, a plain hidden path in one; and `claude/` beside them all.
fn world(test: &str) -> World {
    let w = World::new(test);
    for name in [
        "m2/docs",
        "box/m3",
        "box2/data2",
        "data/agents/claude-acp/home",
        "claude",
        "store",
    ] {
        w.dir.dir(name);
    }
    w
}

/// The builder hiding the data dir, `box2/data2`, and `m2` and `box/m3` as matters.
fn hiding(w: &World) -> PolicyBuilder {
    w.builder()
        .hidden(w.at("data"))
        .hidden(w.at("box2/data2"))
        .hidden_matter(&w.at("m2"), "m2-id")
        .hidden_matter(&w.at("box/m3"), "m3-id")
}

/// `p` with its folder canonical, as the policy holds a path that may not exist.
fn canonish(p: &Path) -> PathBuf {
    canon(p.parent().unwrap()).join(p.file_name().unwrap())
}

/// The refusal must be [`Error::HiddenOverlap`] for `allowance`, naming `path` and `hidden`.
#[track_caller]
fn overlaps(
    result: Result<Policy, Error>,
    allowance: HiddenOverlapAllowance,
    path: &Path,
    hidden: &Path,
) {
    match result {
        Err(Error::HiddenOverlap {
            allowance: got,
            path: got_path,
            hidden: got_hidden,
        }) => {
            assert_eq!(got, allowance);
            assert_eq!(got_path, canonish(path));
            assert_eq!(got_hidden, canonish(hidden));
        }
        other => panic!("expected a hidden overlap of the {allowance}: {other:?}"),
    }
}

#[test]
fn a_claude_store_on_inside_or_above_a_hidden_matter_is_refused() {
    let w = world("overlap-store-matter");
    for (store, matter) in [("m2", "m2"), ("m2/docs", "m2"), ("box", "box/m3")] {
        let result = hiding(&w).claude_store(w.at(store)).build();
        overlaps(result, STORE, &w.at(store), &w.at(matter));
    }
}

#[test]
fn a_claude_stores_beside_lock_on_a_hidden_matter_is_refused() {
    let w = world("overlap-store-beside");
    let result = hiding(&w)
        .hidden_matter(&w.at("store.lock"), "lock-id")
        .claude_store(w.at("store"))
        .build();
    overlaps(result, STORE, &w.at("store.lock"), &w.at("store.lock"));
}

#[test]
fn a_claude_store_on_or_above_a_plain_hidden_path_is_refused() {
    let w = world("overlap-store-plain");
    for (store, hidden) in [("data", "data"), ("box2", "box2/data2")] {
        let result = hiding(&w).claude_store(w.at(store)).build();
        overlaps(result, STORE, &w.at(store), &w.at(hidden));
    }
}

#[test]
fn a_claude_store_inside_a_plain_hidden_path_or_beside_one_builds() {
    let w = world("overlap-store-ok");
    for store in ["data/agents/claude-acp/home", "claude"] {
        let policy = hiding(&w).claude_store(w.at(store)).build().unwrap();
        assert_eq!(policy.claude_store().unwrap().dir(), canon(&w.at(store)));
    }
}

#[test]
fn a_browser_socket_folder_on_inside_or_above_a_hidden_path_is_refused() {
    let w = world("overlap-sockets");
    w.dir.dir("data/run");
    for (dir, hidden) in [
        ("m2", "m2"),
        ("m2/docs", "m2"),
        ("box", "box/m3"),
        ("data", "data"),
        ("data/run", "data"),
        ("box2", "box2/data2"),
    ] {
        let result = hiding(&w).browser_socket_path(canon(&w.at(dir))).build();
        overlaps(result, SOCKETS, &w.at(dir), &w.at(hidden));
    }
}

#[test]
fn a_browser_socket_folder_beside_hidden_paths_builds() {
    let w = world("overlap-sockets-ok");
    let dir = canon(&w.at("claude"));
    let policy = hiding(&w).browser_socket_path(dir.clone()).build().unwrap();
    assert_eq!(policy.browser_socket_dir(), Some(dir.as_path()));
}

#[test]
fn a_hidden_overlap_is_a_failed_launch_naming_both_paths() {
    let error = Error::HiddenOverlap {
        allowance: STORE,
        path: PathBuf::from("/m2"),
        hidden: PathBuf::from("/m2"),
    };
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
    assert_eq!(error.cause(), None);
    assert_eq!(
        error.to_string(),
        "sandbox policy refused: the Claude store folder may not equal, hold or sit inside a \
         hidden path: /m2 and /m2"
    );
}
