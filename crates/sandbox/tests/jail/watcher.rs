//! The denial watcher: a refused read reaches the launch's sink within a second with its path
//! and class; a line a jailed process forges through `logger` does not; Clauseo's own
//! (unjailed) processes report nothing.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use clauseo_sandbox::{Access, Clock, Denial, PathClass, SystemClock};

use crate::support::{World, launcher, run, run_outside};

#[test]
fn a_refused_read_is_reported_within_a_second() {
    let w = World::new();
    let policy = w.builder().build().unwrap();
    let secret = dunce::canonicalize(w.at("outside/secret.txt")).unwrap();
    let hidden = dunce::canonicalize(w.at("data/versions/v1.txt")).unwrap();
    let (launcher, denials) = launcher(policy);
    let m1 = w.at("m1");

    // Unjailed, the same read succeeds and nothing is reported.
    run_outside("/bin/cat", &[secret.to_str().unwrap()], &m1).assert_ok();

    // A jailed process forging a Sandbox line with this launch's tag, through the log.
    let forged = format!(
        "Sandbox: cat(1) deny(1) file-read-data /forged\n{}",
        launcher.tag()
    );
    run(&launcher, "/usr/bin/logger", &[&forged], &m1, &[]);

    run(&launcher, "/bin/cat", &[secret.to_str().unwrap()], &m1, &[]).assert_refused();
    let read = Denial::Path {
        path: secret,
        access: Access::Read,
        class: PathClass::Outside,
    };
    let before = within_a_second(&denials, &read);
    run(&launcher, "/bin/cat", &[hidden.to_str().unwrap()], &m1, &[]).assert_refused();
    let hidden = Denial::Path {
        path: hidden,
        access: Access::Read,
        class: PathClass::Hidden,
    };
    let between = within_a_second(&denials, &hidden);
    for d in before.iter().chain(&between) {
        let Denial::Path { path, .. } = d else {
            continue;
        };
        assert_ne!(path, Path::new("/forged"), "the forged line was reported");
    }
}

/// Waits up to a second, from now, for `expected`; returns what came before it. The jail may
/// also refuse a system library's own probes (CoreFoundation reads `~/.CFUserTextEncoding`).
fn within_a_second(denials: &mpsc::Receiver<Denial>, expected: &Denial) -> Vec<Denial> {
    let clock = SystemClock::new();
    let mut before = Vec::new();
    loop {
        let left = Duration::from_secs(1).saturating_sub(clock.now());
        match denials.recv_timeout(left) {
            Ok(d) if &d == expected => return before,
            Ok(d) => before.push(d),
            Err(error) => panic!("no {expected:?} within 1 s ({error}); got {before:?}"),
        }
    }
}
