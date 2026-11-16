//! The allowed set (PR 2): exact hosts only, swapped whole, a reader keeping its copy.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use clauseo_blocklist::{Allowed, Watcher};

#[test]
fn an_allowed_host_matches_exactly_and_a_swap_keeps_a_readers_copy() {
    let allowed = Allowed::default();
    assert!(!allowed.contains("evil.example"));
    allowed.set(BTreeSet::from(["evil.example".to_owned()]));
    assert!(allowed.contains("evil.example"));
    // Exact: neither a host under it nor one it lies under.
    assert!(!allowed.contains("a.evil.example"));
    assert!(!allowed.contains("example"));
    assert!(!allowed.contains("EVIL.example"));
    let held = allowed.get();
    allowed.set(BTreeSet::new());
    assert!(!allowed.contains("evil.example"));
    assert!(held.contains("evil.example"));
}

#[test]
fn a_watcher_gets_the_removed_hosts_and_a_dropped_one_is_pruned() {
    let allowed = Allowed::default();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let watcher: Arc<Watcher> = Arc::new(move |removed: &BTreeSet<String>| {
        sink.lock().unwrap().push(removed.clone());
    });
    allowed.watch(&watcher);
    let dropped: Arc<Watcher> = Arc::new(|_: &BTreeSet<String>| panic!("dropped watcher ran"));
    allowed.watch(&dropped);
    assert!(
        format!("{allowed:?}").contains("watchers: 2"),
        "{allowed:?}"
    );
    drop(dropped);

    let before = allowed.generation();
    let hosts = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect();
    allowed.set(hosts(&["a.example", "b.example"]));
    // Nothing removed: no call, and the dropped watcher is not pruned yet.
    assert_eq!(*seen.lock().unwrap(), Vec::<BTreeSet<String>>::new());
    allowed.set(hosts(&["b.example", "c.example"]));
    allowed.set(hosts(&[]));
    assert_eq!(
        *seen.lock().unwrap(),
        vec![hosts(&["a.example"]), hosts(&["b.example", "c.example"])]
    );
    assert_eq!(allowed.generation(), before + 3);
    assert!(
        format!("{allowed:?}").contains("watchers: 1"),
        "{allowed:?}"
    );
}
