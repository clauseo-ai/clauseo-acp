//! The allowed sites' saves through the host (decision record 0053, PR 2): each runs on the
//! allowed sites' own thread, in order, hash first, so a Keychain prompt holds up nothing else;
//! a save that fails is answered as not saved, and a revoke that cannot be saved voids the
//! stored hash when it can; a start-up read that never returns gives up after 30 s; only the
//! latest 3 set-aside files are kept.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;

use clauseo_blocklist::Allowed;
use clauseo_connectors::{MemoryStore, SecretStore};
use clauseo_host::AllowedSites;
use clauseo_protocol::{
    AllowScope, CommandEnvelope, CommandOutcome, ErrorCode, HostCommand, HostError,
};

use crate::allowed_sites::{allow, hosts, listed, notice, start_with_secrets};
use crate::allowed_stores::{
    ACCOUNT, AGENTS, Flaky, Gated, allow_in, allow_unsaved, always, file, rejected, replace,
    restart, revoke, saved, start_on,
};
use crate::support::{Dir, FAKE, Running, command_id, matter_with_thread, wait};

/// Sends `command` from a thread of its own, so the test goes on while it waits.
fn send(host: &Running, command: HostCommand) -> JoinHandle<Result<CommandOutcome, HostError>> {
    let handle = host.handle.clone();
    std::thread::spawn(move || {
        let envelope = CommandEnvelope {
            command_id: command_id(),
            command,
        };
        wait("dispatch", handle.dispatch(envelope))
    })
}

/// Waits until `view` holds exactly `want`.
fn until_view(view: &Allowed, want: &[&str]) {
    wait("the view", async {
        while hosts(view) != want {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
}

/// A hash write that waits (a Keychain prompt) holds up no other command: the host answers
/// them, and each allow or revoke takes effect at once and is answered once its save is. Saves
/// keep their order: allow A, revoke A, allow B leaves only B after a restart.
#[test]
fn a_blocked_hash_write_holds_up_nothing_and_saves_in_order() {
    let dir = Dir::new("allowed_saving_order", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let gated = Gated::new(&keychain, true);
    let (host, sites) = start_on(&dir, 0, Arc::clone(&gated) as Arc<dyn SecretStore>);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let view = sites.for_matter(matter);
    let a = notice(&host, (matter, thread), "a.test");
    let b = notice(&host, (matter, thread), "b.test");
    let allowed_a = send(&host, allow(a, AllowScope::Always));
    until_view(&view, &["a.test"]);
    let revoked_a = send(&host, revoke("a.test"));
    until_view(&view, &[]);
    let allowed_b = send(&host, allow(b, AllowScope::Always));
    until_view(&view, &["b.test"]);
    // Other commands go on meanwhile.
    matter_with_thread(&dir, &host, "n", &[], FAKE);
    assert_eq!(listed(&host), [always("b.test")]);
    assert!(!allowed_a.is_finished() && !revoked_a.is_finished() && !allowed_b.is_finished());

    gated.release();
    for sent in [allowed_a, revoked_a, allowed_b] {
        assert_eq!(sent.join().unwrap().unwrap(), CommandOutcome::Done);
    }
    host.shutdown();
    let (host, _) = restart(&dir, 1, &keychain);
    assert_eq!(listed(&host), [always("b.test")]);
    host.shutdown();
}

/// A revoke whose hash cannot be set deletes the stored hash, so the next start honours no
/// allows rather than the revoked one; when the delete fails too, the revoke answers
/// `blocklist.revoke_not_saved` and the old pair is honoured again after a restart (the
/// residual the answer warns of).
#[test]
fn a_revoke_not_saved_voids_the_hash_or_says_it_may_return() {
    let dir = Dir::new("allowed_saving_void", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let flaky = Flaky::new(&keychain);
    let (host, _) = start_on(&dir, 0, Arc::clone(&flaky) as Arc<dyn SecretStore>);
    allow_in(&dir, &host, "m", "a.test");
    allow_in(&dir, &host, "n", "b.test");
    flaky.fail_set.store(true, Ordering::SeqCst);
    assert_eq!(host.ok(revoke("a.test")), CommandOutcome::Done);
    assert_eq!(keychain.peek(ACCOUNT), None);
    assert_eq!(saved(&host), [false]);
    host.shutdown();
    let (host, _) = restart(&dir, 1, &keychain);
    assert_eq!(listed(&host), []);
    allow_in(&dir, &host, "o", "a.test");
    host.shutdown();

    let flaky = Flaky::new(&keychain);
    let (host, _) = start_on(&dir, 2, Arc::clone(&flaky) as Arc<dyn SecretStore>);
    flaky.fail_set.store(true, Ordering::SeqCst);
    flaky.fail_delete.store(true, Ordering::SeqCst);
    host.refused(revoke("a.test"), ErrorCode::BlocklistRevokeNotSaved);
    assert_eq!(listed(&host), []);
    // An allow in the same run still says it is the allow that was not saved.
    allow_unsaved(&dir, &host, "p", "c.test");
    host.shutdown();
    let (host, _) = restart(&dir, 3, &keychain);
    assert_eq!(listed(&host), [always("a.test")]);
    host.shutdown();
}

/// The hash set but not its file (a failed write here; a crash between the two leaves the
/// same): the allow is answered as not saved, and the next start honours no allows. Skipped as
/// root, whom a read-only folder does not stop.
#[cfg(unix)]
#[test]
fn a_hash_without_its_file_honours_no_allows() {
    if nix::unistd::geteuid().is_root() {
        return;
    }
    let dir = Dir::new("allowed_saving_crash", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    allow_in(&dir, &host, "m", "a.test");
    let folder = dir.data().join("blocklist");
    crate::write::set_mode(&folder, 0o555);
    allow_unsaved(&dir, &host, "n", "b.test");
    crate::write::set_mode(&folder, 0o700);
    assert_eq!(saved(&host), [false, false]);
    host.shutdown();
    let (host, _) = restart(&dir, 1, &keychain);
    assert_eq!(listed(&host), []);
    host.shutdown();
}

/// A start-up read that never returns (a prompt left open) is given up on after 30 s: no
/// stored allows, a held allow answered as not saved, and the read ignored when it lands.
#[test]
fn a_start_up_read_that_never_returns_is_given_up_on() {
    let dir = Dir::new("allowed_saving_timeout", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    allow_in(&dir, &host, "m", "a.test");
    host.shutdown();
    let before = std::fs::read(file(&dir)).unwrap();

    let gated = Gated::new(&keychain, false);
    let sites = Arc::new(AllowedSites::default());
    let secrets: Arc<dyn SecretStore> = Arc::clone(&gated) as Arc<dyn SecretStore>;
    let host = start_with_secrets(&dir, 1, &sites, Some(secrets));
    let (matter, thread, _) = matter_with_thread(&dir, &host, "n", &[], FAKE);
    let id = notice(&host, (matter, thread), "b.test");
    let held = send(&host, allow(id, AllowScope::Always));
    host.settle();
    assert!(!sites.loaded() && !held.is_finished());
    host.clock.advance(Duration::from_secs(30));
    let answer = held.join().unwrap();
    assert_eq!(
        answer.unwrap_err().code(),
        ErrorCode::BlocklistAllowNotSaved
    );
    assert!(sites.loaded());
    assert_eq!(listed(&host), [always("b.test")]);
    gated.release();
    host.settle();
    assert_eq!(listed(&host), [always("b.test")]);
    assert_eq!(saved(&host), [false]);
    assert_eq!(std::fs::read(file(&dir)).unwrap(), before);
    host.shutdown();
}

/// Setting a file aside keeps only the latest 3 set-aside files.
#[test]
fn only_the_latest_three_set_aside_files_are_kept() {
    let dir = Dir::new("allowed_saving_prune", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let folder = dir.data().join("blocklist");
    let old = [
        "allowed.json.rejected-20250101T000000Z",
        "allowed.json.rejected-20250102T000000Z",
        "allowed.json.rejected-20250102T000000Z-2",
        "allowed.json.rejected-20250102T000000Z-10",
    ];
    for name in old {
        replace(&folder.join(name), b"old");
    }
    replace(&file(&dir), b"{\"v\":1,\"always\":{\"c2.test\":1}}");
    let (host, _) = restart(&dir, 0, &keychain);
    assert_eq!(listed(&host), []);
    allow_in(&dir, &host, "m", "a.test");
    let kept = rejected(&dir);
    assert_eq!(kept.len(), 3, "{kept:?}");
    assert!(kept.contains(&old[2].to_owned()) && kept.contains(&old[3].to_owned()));
    assert!(!kept.contains(&old[0].to_owned()) && !kept.contains(&old[1].to_owned()));
    host.shutdown();
}

/// Stops `host`, moving its clock on while the stop waits.
fn shutdown_advancing(host: Running) {
    let clock = host.clock.clone();
    let stopping = std::thread::spawn(move || host.shutdown());
    wait("the stop", async {
        while !stopping.is_finished() {
            clock.advance(Duration::from_millis(500));
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
    stopping.join().unwrap();
}

/// A quit while a revoke's save is stuck (a prompt left open) waits 2 s, then deletes the
/// stored hash as its last step, so the next start honours no allows, the revoked site
/// included.
#[test]
fn a_quit_with_an_unsaved_revoke_voids_the_hash() {
    let dir = Dir::new("allowed_saving_quit", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    let matter = allow_in(&dir, &host, "m", "a.test");
    host.shutdown();

    let gated = Gated::new(&keychain, true);
    let (host, sites) = start_on(&dir, 1, Arc::clone(&gated) as Arc<dyn SecretStore>);
    let view = sites.for_matter(matter);
    until_view(&view, &["a.test"]);
    let revoked = send(&host, revoke("a.test"));
    until_view(&view, &[]);
    shutdown_advancing(host);
    drop(revoked.join().unwrap());
    assert_eq!(keychain.peek(ACCOUNT), None);
    // The stuck save finishing later writes no file: still no matching pair.
    gated.release();
    let (host, _) = restart(&dir, 2, &keychain);
    assert_eq!(listed(&host), []);
    host.shutdown();
}

/// A quit with nothing queued deletes nothing: the next start honours what was saved.
#[test]
fn a_quit_with_nothing_queued_deletes_nothing() {
    let dir = Dir::new("allowed_saving_quit_clean", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    allow_in(&dir, &host, "m", "a.test");
    host.shutdown();
    assert_eq!(keychain.deletes(), 0);
    let (host, _) = restart(&dir, 1, &keychain);
    assert_eq!(listed(&host), [always("a.test")]);
    host.shutdown();
}

/// A save stuck at the quit whose state equals the file's (here a revoke that removed nothing)
/// lands after the stop deleted the hash: the hash it sets would match the old file, so it is
/// deleted again, and the next start has no allows.
#[test]
fn a_stuck_save_landing_after_the_stop_leaves_no_matching_pair() {
    let dir = Dir::new("allowed_saving_late_set", AGENTS);
    let keychain = Arc::new(MemoryStore::new());
    let (host, _) = restart(&dir, 0, &keychain);
    let matter = allow_in(&dir, &host, "m", "a.test");
    host.shutdown();
    let before = std::fs::read(file(&dir)).unwrap();

    let gated = Gated::new(&keychain, true);
    let (host, sites) = start_on(&dir, 1, Arc::clone(&gated) as Arc<dyn SecretStore>);
    until_view(&sites.for_matter(matter), &["a.test"]);
    let revoked = send(&host, revoke("nothing.test"));
    wait("the stuck save", async {
        while !gated.stuck() {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
    shutdown_advancing(host);
    drop(revoked.join().unwrap());
    assert_eq!(keychain.deletes(), 1);
    gated.release();
    wait("the late save", async {
        while keychain.deletes() < 2 {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
    assert_eq!(keychain.writes(), 2);
    assert_eq!(keychain.peek(ACCOUNT), None);
    assert_eq!(std::fs::read(file(&dir)).unwrap(), before);
    let (host, _) = restart(&dir, 2, &keychain);
    assert_eq!(listed(&host), []);
    host.shutdown();
}
