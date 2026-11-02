//! The denial watcher: a refused read reaches the launch's sink within a second with its path
//! and class; a line a jailed process forges through `logger` does not; a denial from a
//! Seatbelt profile with another, unregistered Clauseo tag does not; CoreFoundation's read of
//! the text-encoding preference is not refused.

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

    // Unjailed, the same read succeeds. Then the watcher starts with this launcher's first
    // launch.
    run_outside("/bin/cat", &[secret.to_str().unwrap()], &m1).assert_ok();
    run(&launcher, "/usr/bin/true", &[], &m1, &[]).assert_ok();

    // A process in a Seatbelt profile of its own, with a Clauseo tag no launcher registered:
    // the kernel refuses and logs it, and the watcher must not report it to this launch.
    let unregistered = dunce::canonicalize(w.file("outside/unregistered.txt", "u\n")).unwrap();
    let profile = r#"(version 1)(allow default)(deny file-read-data (literal (param "P")) (with message "clauseo-unregistered-1"))"#;
    let define = format!("P={}", unregistered.display());
    run_outside(
        "/usr/bin/sandbox-exec",
        &[
            "-p",
            profile,
            "-D",
            &define,
            "/bin/cat",
            unregistered.to_str().unwrap(),
        ],
        &m1,
    )
    .assert_refused();

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
        assert_ne!(path, &unregistered, "another tag's denial was reported");
    }
}

/// What a jailed `nc` (which loads CoreFoundation) gets refused before a marker read, with
/// `policy_home` as the policy's home and `home/` as CoreFoundation's (`CFFIXED_USER_HOME`).
fn encoding_denials(w: &World, policy_home: &str) -> Vec<Denial> {
    let policy = w.builder().home(w.at(policy_home)).build().unwrap();
    let secret = dunce::canonicalize(w.at("outside/secret.txt")).unwrap();
    let (launcher, denials) = launcher(policy);
    let m1 = w.at("m1");
    let home = w.at("home");
    // An inherited `__CF_USER_TEXT_ENCODING` (launchd sets it for most apps) spares
    // CoreFoundation the read; empty, it reads the file.
    let env = [
        ("CFFIXED_USER_HOME", home.to_str().unwrap()),
        ("__CF_USER_TEXT_ENCODING", ""),
    ];
    // nc loads CoreFoundation, which reads the preference at start.
    run(
        &launcher,
        "/usr/bin/nc",
        &["-z", "-G", "1", "127.0.0.1", "9"],
        &m1,
        &env,
    );
    run(&launcher, "/bin/cat", &[secret.to_str().unwrap()], &m1, &[]).assert_refused();
    let marker = Denial::Path {
        path: secret,
        access: Access::Read,
        class: PathClass::Outside,
    };
    within_a_second(&denials, &marker)
}

#[test]
fn the_text_encoding_preference_is_readable() {
    let w = World::new();
    w.file("home/.CFUserTextEncoding", "0x08000100:0x0\n");
    let wanted = dunce::canonicalize(w.at("home/.CFUserTextEncoding")).unwrap();
    let is_encoding = |d: &Denial| matches!(d, Denial::Path { path, .. } if path == &wanted);
    // The policy's home is the one CoreFoundation reads: nothing is refused.
    let quiet = encoding_denials(&w, "home");
    assert!(!quiet.iter().any(is_encoding), "{quiet:?}");
    // Control: with the policy's home elsewhere, the same read is refused and reported, so the
    // check above is not empty.
    w.dir("other-home");
    let loud = encoding_denials(&w, "other-home");
    assert!(loud.iter().any(is_encoding), "{loud:?}");
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
