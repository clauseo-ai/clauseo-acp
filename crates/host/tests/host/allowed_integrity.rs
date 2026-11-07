//! `allowed.json`'s integrity check (decision record 0053, PR 2) through the host: the file's
//! sha256 in the OS secret store (a `MemoryStore` here) decides whether its allows are trusted
//! at the start; a tampered, rolled-back or unhashed file allows nothing and is set aside at
//! the run's first write; a store that fails is asked once and the run goes on in memory.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use clauseo_connectors::{MemoryStore, SecretStore};
use clauseo_host::AllowedSites;
use clauseo_protocol::{AllowScope, CommandEnvelope, CommandOutcome, ErrorCode};

use crate::allowed_sites::{allow, hosts, listed, notice, start_with_secrets, wait_loaded};
use crate::allowed_stores::{
    ACCOUNT, AGENTS, Denied, Gated, allow_in, allow_unsaved, always, file, hex, rejected, replace,
    restart, revoke, saved,
};
use crate::support::{Dir, FAKE, command_id, matter_with_thread, wait};

#[test]
fn the_hash_round_trips_and_a_tampered_file_is_set_aside_at_the_first_write() {
    let dir = Dir::new("allowed_integrity_tamper", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    let matter = allow_in(&dir, &host, "m", "evil.example");
    let written = std::fs::read(file(&dir)).unwrap();
    assert_eq!(keychain.peek(ACCOUNT), Some(hex(&written).into_bytes()));
    host.shutdown();

    // A clean start trusts it, and its next write sets nothing aside.
    let (host, sites) = restart(&dir, 1, &keychain);
    assert_eq!(hosts(&sites.for_matter(matter)), ["evil.example"]);
    allow_in(&dir, &host, "n", "phish.test");
    assert_eq!(rejected(&dir), Vec::<String>::new());
    host.shutdown();

    // An edit that adds a host allows nothing; the file stays until the first write.
    let text = String::from_utf8(std::fs::read(file(&dir)).unwrap()).unwrap();
    let tampered = text.replace("\"always\":{", "\"always\":{\"c2.test\":1,");
    replace(&file(&dir), tampered.as_bytes());
    let (host, sites) = restart(&dir, 2, &keychain);
    assert_eq!(listed(&host), []);
    assert_eq!(hosts(&sites.for_matter(matter)), Vec::<String>::new());
    assert_eq!(std::fs::read(file(&dir)).unwrap(), tampered.as_bytes());
    assert_eq!(rejected(&dir), Vec::<String>::new());
    allow_in(&dir, &host, "o", "new.test");
    let aside = rejected(&dir);
    assert_eq!(aside.len(), 1, "{aside:?}");
    let kept = std::fs::read(dir.data().join("blocklist").join(&aside[0])).unwrap();
    assert_eq!(kept, tampered.as_bytes());
    assert_eq!(listed(&host), [always("new.test")]);
    host.shutdown();
    let (host, _) = restart(&dir, 3, &keychain);
    assert_eq!(listed(&host), [always("new.test")]);
    host.shutdown();
}

#[test]
fn a_file_without_its_hash_or_rolled_back_allows_nothing() {
    let dir = Dir::new("allowed_integrity_rollback", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    allow_in(&dir, &host, "m", "a.test");
    let saved_a = std::fs::read(file(&dir)).unwrap();
    allow_in(&dir, &host, "n", "b.test");
    host.ok(revoke("a.test"));
    host.shutdown();

    // Rollback: A's file, validly hashed once, put back over B's.
    replace(&file(&dir), &saved_a);
    let (host, _) = restart(&dir, 1, &keychain);
    assert_eq!(listed(&host), []);
    host.shutdown();

    // No item while the file exists.
    keychain.delete(ACCOUNT).unwrap();
    let (host, _) = restart(&dir, 2, &keychain);
    assert_eq!(listed(&host), []);
    host.shutdown();
}

#[test]
fn a_revoke_updates_the_hash_and_stays_revoked_after_a_restart() {
    let dir = Dir::new("allowed_integrity_revoke", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    allow_in(&dir, &host, "m", "a.test");
    allow_in(&dir, &host, "n", "b.test");
    host.shutdown();
    let (host, _) = restart(&dir, 1, &keychain);
    assert_eq!(host.ok(revoke("a.test")), CommandOutcome::Done);
    let written = std::fs::read(file(&dir)).unwrap();
    assert_eq!(keychain.peek(ACCOUNT), Some(hex(&written).into_bytes()));
    host.shutdown();
    let (host, _) = restart(&dir, 2, &keychain);
    assert_eq!(listed(&host), [always("b.test")]);
    host.shutdown();
}

#[test]
fn a_store_that_fails_at_the_start_is_asked_once_and_the_run_goes_on_in_memory() {
    // Each change applies, answered as not saved; a revoke tries the hash's delete once.
    let dir = Dir::new("allowed_integrity_denied", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    allow_in(&dir, &host, "m", "a.test");
    host.shutdown();
    let before = std::fs::read(file(&dir)).unwrap();

    let denied = Arc::new(Denied::default());
    let sites = Arc::new(AllowedSites::default());
    let secrets: Arc<dyn SecretStore> = Arc::clone(&denied) as Arc<dyn SecretStore>;
    let host = start_with_secrets(&dir, 1, &sites, Some(secrets));
    wait_loaded(&sites);
    assert_eq!(listed(&host), []);
    let matter = allow_unsaved(&dir, &host, "n", "b.test");
    assert_eq!(hosts(&sites.for_matter(matter)), ["b.test"]);
    host.refused(revoke("b.test"), ErrorCode::BlocklistRevokeNotSaved);
    assert_eq!(hosts(&sites.for_matter(matter)), Vec::<String>::new());
    host.refused(revoke("b.test"), ErrorCode::BlocklistRevokeNotSaved);
    allow_unsaved(&dir, &host, "o", "c.test");
    assert_eq!(listed(&host), [always("c.test")]);
    assert_eq!(saved(&host), [false]);
    // The read, then one delete: never asked again.
    assert_eq!(denied.calls.load(Ordering::SeqCst), 2);
    // Nothing written, nothing set aside: the earlier run's file and hash still agree.
    assert_eq!(std::fs::read(file(&dir)).unwrap(), before);
    assert_eq!(rejected(&dir), Vec::<String>::new());
    host.shutdown();
    let (host, _) = restart(&dir, 2, &keychain);
    assert_eq!(listed(&host), [always("a.test")]);
    host.shutdown();

    // No seam at all: allows are not saved, but revokes need no saving.
    let sites = Arc::new(AllowedSites::default());
    let host = start_with_secrets(&dir, 3, &sites, None);
    wait_loaded(&sites);
    assert_eq!(listed(&host), []);
    allow_unsaved(&dir, &host, "p", "d.test");
    assert_eq!(listed(&host), [always("d.test")]);
    assert_eq!(saved(&host), [false]);
    // Without a store no start honours an allow, so a revoke cannot come back: `Done`.
    assert_eq!(host.ok(revoke("d.test")), CommandOutcome::Done);
    assert_eq!(listed(&host), []);
    host.shutdown();
}

/// A hash that is not set writes nothing: the allow applies for the run, answered as not saved
/// and listed unsaved, while the earlier saved pair stays as it was.
#[test]
fn a_hash_that_fails_to_save_keeps_the_allow_for_the_run_only() {
    let dir = Dir::new("allowed_integrity_set_fails", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, sites) = restart(&dir, 0, &keychain);
    allow_in(&dir, &host, "m", "a.test");
    let before = std::fs::read(file(&dir)).unwrap();
    keychain.fail_next_writes(1);
    let matter = allow_unsaved(&dir, &host, "n", "b.test");
    assert_eq!(hosts(&sites.for_matter(matter)), ["a.test", "b.test"]);
    assert_eq!(listed(&host), [always("a.test"), always("b.test")]);
    assert_eq!(saved(&host), [true, false]);
    assert_eq!(std::fs::read(file(&dir)).unwrap(), before);
    // Nothing more is written this run, though the store would take it now.
    allow_unsaved(&dir, &host, "o", "c.test");
    assert_eq!(std::fs::read(file(&dir)).unwrap(), before);
    host.shutdown();
    let (host, _) = restart(&dir, 1, &keychain);
    assert_eq!(listed(&host), [always("a.test")]);
    host.shutdown();
}

#[test]
fn a_file_over_the_cap_is_set_aside_at_a_revoke() {
    let dir = Dir::new("allowed_integrity_cap", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    allow_in(&dir, &host, "m", "a.test");
    host.shutdown();
    let mut big = std::fs::read(file(&dir)).unwrap();
    big.resize(4 * 1024 * 1024 + 1, b' ');
    replace(&file(&dir), &big);
    keychain.put(ACCOUNT, hex(&big).as_bytes());

    let (host, _) = restart(&dir, 1, &keychain);
    assert_eq!(listed(&host), []);
    assert_eq!(host.ok(revoke("a.test")), CommandOutcome::Done);
    let aside = rejected(&dir);
    assert_eq!(aside.len(), 1, "{aside:?}");
    assert_eq!(
        std::fs::read(dir.data().join("blocklist").join(&aside[0])).unwrap(),
        big
    );
    let written = std::fs::read(file(&dir)).unwrap();
    assert_eq!(written, br#"{"v":1,"always":{},"matters":{}}"#);
    assert_eq!(keychain.peek(ACCOUNT), Some(hex(&written).into_bytes()));
    host.shutdown();
}

/// Once 10,000 allows are kept a new one is `command.invalid` and changes nothing; allowing one
/// already kept still answers `Done`.
#[test]
fn a_new_allow_past_the_most_is_refused() {
    let dir = Dir::new("allowed_integrity_most", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let hosts: Vec<String> = (0..10_000).map(|n| format!("\"h{n:05}.test\":1")).collect();
    let full = format!(
        r#"{{"v":1,"always":{{{}}},"matters":{{}}}}"#,
        hosts.join(",")
    );
    replace(&file(&dir), full.as_bytes());
    keychain.put(ACCOUNT, hex(full.as_bytes()).as_bytes());
    let (host, _) = restart(&dir, 0, &keychain);
    assert_eq!(listed(&host).len(), 10_000);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let id = notice(&host, (matter, thread), "one-more.test");
    host.refused(allow(id, AllowScope::Always), ErrorCode::CommandInvalid);
    let id = notice(&host, (matter, thread), "h00042.test");
    assert_eq!(host.ok(allow(id, AllowScope::Always)), CommandOutcome::Done);
    assert_eq!(listed(&host).len(), 10_000);
    assert_eq!(std::fs::read(file(&dir)).unwrap(), full.as_bytes());
    host.shutdown();
}

/// A rejected file that cannot be set aside is never overwritten; the click still succeeds and
/// the allow applies for the run. Skipped as root, whom a read-only folder does not stop.
#[cfg(unix)]
#[test]
fn a_failed_set_aside_overwrites_nothing() {
    if nix::unistd::geteuid().is_root() {
        return;
    }
    let dir = Dir::new("allowed_integrity_no_rename", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    allow_in(&dir, &host, "m", "a.test");
    host.shutdown();
    keychain.delete(ACCOUNT).unwrap();
    let before = std::fs::read(file(&dir)).unwrap();

    let (host, sites) = restart(&dir, 1, &keychain);
    let folder = dir.data().join("blocklist");
    crate::write::set_mode(&folder, 0o555);
    let matter = allow_unsaved(&dir, &host, "n", "b.test");
    crate::write::set_mode(&folder, 0o700);
    assert_eq!(hosts(&sites.for_matter(matter)), ["b.test"]);
    assert_eq!(std::fs::read(file(&dir)).unwrap(), before);
    assert_eq!(rejected(&dir), Vec::<String>::new());
    // In memory only from then on, even with the folder writable again.
    allow_unsaved(&dir, &host, "o", "c.test");
    assert_eq!(std::fs::read(file(&dir)).unwrap(), before);
    assert_eq!(keychain.peek(ACCOUNT), None);
    host.shutdown();
}

/// A store read that waits (a Keychain prompt) holds up only the allowed sites: the host starts
/// and answers other commands, every view stays empty and the query lists none, and an allow
/// sent meanwhile is answered once the read is back, and applies.
#[test]
fn a_slow_store_read_holds_up_only_the_allowed_sites() {
    let dir = Dir::new("allowed_integrity_pending", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    let matter = allow_in(&dir, &host, "m", "a.test");
    host.shutdown();
    let reads = keychain.reads();

    let gated = Gated::new(&keychain, false);
    let sites = Arc::new(AllowedSites::default());
    let secrets: Arc<dyn SecretStore> = Arc::clone(&gated) as Arc<dyn SecretStore>;
    let host = start_with_secrets(&dir, 1, &sites, Some(secrets));
    let view = sites.for_matter(matter);
    let (other, thread, _) = matter_with_thread(&dir, &host, "n", &[], FAKE);
    assert_eq!(listed(&host), []);
    assert!(!sites.loaded());
    let id = notice(&host, (other, thread), "b.test");
    let handle = host.handle.clone();
    let sent = std::thread::spawn(move || {
        let envelope = CommandEnvelope {
            command_id: command_id(),
            command: allow(id, AllowScope::Always),
        };
        wait("dispatch", handle.dispatch(envelope))
    });
    wait(
        "a pause",
        async_io::Timer::after(Duration::from_millis(200)),
    );
    assert!(!sent.is_finished());
    host.settle();
    assert_eq!(hosts(&view), Vec::<String>::new());
    assert_eq!(keychain.reads(), reads);

    gated.release();
    assert_eq!(sent.join().unwrap().unwrap(), CommandOutcome::Done);
    assert!(sites.loaded());
    assert_eq!(hosts(&view), ["a.test", "b.test"]);
    assert_eq!(listed(&host), [always("a.test"), always("b.test")]);
    assert_eq!(keychain.reads(), reads + 1);
    host.shutdown();
}
