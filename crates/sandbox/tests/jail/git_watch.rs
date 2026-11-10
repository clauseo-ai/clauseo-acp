//! The lawyer-side git watch with a real jail (decision record 0038): no repository is made in
//! the session's temp folder or notes, and one renamed into notes is set aside; the first find
//! ends every process of the session, one that left the group with `setsid` included, before
//! anything moves; and a matter on another volume sets aside on that volume, in a `.clauseo`
//! folder the agent can't reach; nothing git reads is made at a launch repository's
//! `commondir` target; and the session's probe can't be shed from inside (no nested
//! `sandbox-exec`, no `launchctl submit`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_sandbox::{Clock as _, Denial, GitItem, GitOutcome, SystemClock};

use crate::support::{Output, World, dir_at, launcher, run_outside, sh};

/// The first git set-aside notice within 5 s.
fn notice(denials: &std::sync::mpsc::Receiver<Denial>) -> (PathBuf, GitItem, GitOutcome) {
    loop {
        match denials.recv_timeout(Duration::from_secs(5)) {
            Ok(Denial::GitSetAside {
                path,
                what,
                outcome,
                ..
            }) => return (path, what, outcome),
            Ok(_) => {}
            Err(error) => panic!("no set-aside within 5 s: {error}"),
        }
    }
}

#[test]
fn no_repository_is_made_in_the_session_temp_folder_or_notes() {
    let w = World::new();
    w.dir("data/run/s1/tmp");
    let policy = w.builder().tmpdir(w.at("data/run/s1/tmp")).build().unwrap();
    let notes = w.at("data/agent-state/m1/notes");
    let notes = notes.to_str().unwrap();
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    for script in [
        r#"mkdir "$TMPDIR/.git""#,
        r#"mkdir -p "$TMPDIR/r" && touch "$TMPDIR/r/HEAD""#,
        r#"mkdir -p "$TMPDIR/r" && touch "$TMPDIR/r/x" && mv "$TMPDIR/r/x" "$TMPDIR/r/head""#,
        r#"mkdir "$1/.GIT""#,
        r#"touch "$1/HEAD""#,
    ] {
        sh(&launcher, &m1, script, &[notes]).assert_refused();
    }
    sh(
        &launcher,
        &m1,
        r#"touch "$TMPDIR/.gitignore" "$1/HEADER.md""#,
        &[notes],
    )
    .assert_ok();
}

/// The notes folder is watched as the matter is: a repository made in the state folder and
/// renamed into notes under another name (which the profile's name rules can't see) is set
/// aside after the run.
#[test]
fn a_repository_renamed_into_notes_is_set_aside() {
    let w = World::new();
    let policy = w.builder().build().unwrap();
    let state = policy.state().to_string_lossy().into_owned();
    let notes = policy.notes().unwrap().to_owned();
    let (launcher, denials) = launcher(policy);
    let script = r#"
        mkdir -p "$1/foo/.git" && echo "ref: refs/heads/main" > "$1/foo/.git/HEAD" || exit 4
        mv "$1/foo" "$2/foo" || exit 5
    "#;
    let out = sh(
        &launcher,
        &w.at("m1"),
        script,
        &[&state, notes.to_str().unwrap()],
    );
    assert!(!matches!(out.code, Some(4 | 5)), "{out:#?}");
    let (path, what, outcome) = notice(&denials);
    assert_eq!((path, what), (PathBuf::from("foo/.git"), GitItem::DotGit));
    assert!(matches!(outcome, GitOutcome::Moved(_)), "{outcome:?}");
    assert!(!notes.join("foo/.git").exists());
    assert!(notes.join("foo").is_dir());
}

/// A repository moved in while a helper that called `setsid` sleeps: the watch ends the shell
/// and the helper (found by the launch's Seatbelt probe, not by its group or parent), and only
/// then sets the repository aside. The helper holds the shell's stdout, so the run would wait
/// its whole 60 s if it lived.
#[test]
fn a_find_ends_a_setsid_helper_before_the_set_aside() {
    let w = World::new();
    let policy = w.builder().build().unwrap();
    let state = policy.state().to_string_lossy().into_owned();
    let (launcher, denials) = launcher(policy);
    let m1 = w.at("m1");
    let script = r#"
        /usr/bin/perl -e 'use POSIX; my $p = fork(); if ($p == 0) { POSIX::setsid();
            exec "/bin/sleep", "60" } open(my $f, ">", "escapee") or die; print $f $p;' || exit 3
        mkdir -p "$1/foo/.git" && echo "ref: refs/heads/main" > "$1/foo/.git/HEAD" || exit 4
        mv "$1/foo" foo || exit 5
        exec /bin/sleep 60
    "#;
    let clock = SystemClock::new();
    let out: Output = sh(&launcher, &m1, script, &[&state]);
    let took = clock.now();
    assert!(
        took < Duration::from_secs(20),
        "the session lived {took:?}: {out:#?}"
    );
    assert_ne!(out.code, Some(0), "{out:#?}");
    let escapee = std::fs::read_to_string(m1.join("escapee")).unwrap();
    let gone = run_outside("/bin/kill", &["-0", escapee.trim()], &m1);
    assert_ne!(gone.code, Some(0), "the setsid helper {escapee} still runs");
    let (path, what, outcome) = notice(&denials);
    assert_eq!((path, what), (PathBuf::from("foo/.git"), GitItem::DotGit));
    assert!(matches!(outcome, GitOutcome::Moved(_)), "{outcome:?}");
    assert!(!m1.join("foo/.git").exists());
}

/// A launch repository's `commondir` naming the matter, or a path not there yet: the agent
/// can't make what git would read from it, in any letter case.
#[test]
fn nothing_git_reads_is_made_at_a_launch_commondir_target() {
    let w = World::new();
    w.file("m1/wt/HEAD", "ref: refs/heads/main\n");
    w.file("m1/wt/commondir", "..\n");
    w.file("m1/lone/HEAD", "ref: refs/heads/main\n");
    w.file("m1/lone/commondir", "../store\n");
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    for script in [
        "mkdir objects",
        "mkdir OBJECTS",
        "mkdir -p refs/heads",
        "touch config",
        "touch packed-refs",
        "mkdir store",
    ] {
        sh(&launcher, &m1, script, &[]).assert_refused();
    }
    sh(
        &launcher,
        &m1,
        "mkdir notes && touch brief.txt store.txt",
        &[],
    )
    .assert_ok();
}

/// A disk image mounted for one test, detached on drop.
struct Volume(PathBuf);

impl Volume {
    fn new(w: &World) -> Volume {
        let image = w.at("vol.dmg");
        let image = image.to_str().unwrap();
        let root = w.at("");
        let made = [
            "create", "-quiet", "-size", "8m", "-fs", "APFS", "-volname", "cgw", image,
        ];
        run_outside("/usr/bin/hdiutil", &made, &root).assert_ok();
        let attach = ["attach", "-nobrowse", "-mountrandom", "/private/tmp", image];
        let out = run_outside("/usr/bin/hdiutil", &attach, &root);
        let mount = out
            .assert_ok()
            .stdout
            .lines()
            .find_map(|line| line.find("/private/tmp/dmg.").map(|at| &line[at..]))
            .map(|at| PathBuf::from(at.trim()));
        Volume(mount.unwrap_or_else(|| panic!("no mount point: {out:#?}")))
    }
}

impl Drop for Volume {
    fn drop(&mut self) {
        let at = self.0.to_str().unwrap();
        drop(run_outside(
            "/usr/bin/hdiutil",
            &["detach", "-force", at],
            Path::new("/"),
        ));
    }
}

#[test]
fn a_matter_on_another_volume_sets_aside_on_that_volume() {
    let w = World::new();
    let volume = Volume::new(&w);
    let m1 = volume.0.join("m1");
    dir_at(&m1);
    let policy = w.builder_at(&m1).build().unwrap();
    let folder = volume.0.join(".clauseo");
    assert_eq!(policy.volume_set_aside(), Some(folder.as_path()));
    let state = policy.state().to_string_lossy().into_owned();
    let (launcher, denials) = launcher(policy);
    // The agent can't reach the volume's set-aside folder.
    let at = folder.to_str().unwrap();
    for script in [
        r#"ls "$1""#,
        r#"mkdir "$1/x""#,
        r#"touch "$1/git-set-aside/x""#,
    ] {
        sh(&launcher, &m1, script, &[at]).assert_refused();
    }
    // A link out of the matter is set aside on the matter's own volume. The shell exits at once,
    // so the final scan takes it: on a disk image mounted a moment before, FSEvents has been seen
    // to deliver nothing for over 10 s under a full parallel test run, and the kill on detect is
    // the main volume's test above.
    sh(&launcher, &m1, r#"ln -s "$1" out"#, &[&state]).assert_ok();
    let (path, what, outcome) = notice(&denials);
    assert_eq!((path, what), (PathBuf::from("out"), GitItem::OutsideLink));
    let GitOutcome::Moved(to) = outcome else {
        panic!("{outcome:?}");
    };
    assert!(
        to.starts_with(folder.join("git-set-aside")),
        "{}",
        to.display()
    );
    assert!(std::fs::symlink_metadata(&to).unwrap().is_symlink());
    assert!(std::fs::symlink_metadata(m1.join("out")).is_err());
}

/// The probe that finds a session's processes can't be shed from inside the jail: a nested
/// `sandbox-exec` can't apply a profile of its own, and `launchctl submit` can't hand launchd
/// a job, which would run outside every jail.
#[test]
fn the_session_probe_cant_be_shed_from_inside() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let m1 = w.at("m1");
    let nested = "/usr/bin/sandbox-exec -p '(version 1)(allow default)' /bin/sleep 61";
    sh(&launcher, &m1, nested, &[]).assert_refused();
    let label = format!("chat.clauseo.test.submit-{}", std::process::id());
    let submit = r#"exec /bin/launchctl submit -l "$1" -- /bin/sleep 61"#;
    let out = sh(&launcher, &m1, submit, &[&label]);
    let listed = run_outside("/bin/launchctl", &["list", &label], &m1);
    if listed.code == Some(0) {
        drop(run_outside("/bin/launchctl", &["remove", &label], &m1));
    }
    assert_ne!(
        listed.code,
        Some(0),
        "launchd took a job from the jail: {out:#?}"
    );
    assert_ne!(out.code, Some(0), "{out:#?}");
}

/// The matter's mounts (`mount` run outside), at or below `at`.
fn mounts_below(at: &Path) -> Vec<String> {
    let at = at.to_str().unwrap();
    run_outside("/sbin/mount", &[], Path::new("/"))
        .stdout
        .lines()
        .filter(|line| line.contains(at) || line.contains("cgw-mount"))
        .map(str::to_owned)
        .collect()
}

/// The kernel's Sandbox lines (`log stream`, outside the jail) while `during` runs: the stream
/// starts first (it prints its header before the kernel's lines reach it), and stops 3 s after
/// `during` returns.
fn sandbox_lines<T>(w: &World, during: impl FnOnce() -> T) -> (T, String) {
    let script = "/usr/bin/log stream --style compact --predicate 'sender == \"Sandbox\"' & \
                  p=$!; sleep 3; : > ready; while [ ! -e stop ]; do sleep 0.2; done; sleep 3; \
                  kill $p";
    let dir = w.dir("log");
    let at = dir.clone();
    let stream = std::thread::spawn(move || run_outside("/bin/sh", &["-c", script], &at));
    let clock = SystemClock::new();
    while !dir.join("ready").exists() && clock.now() < Duration::from_secs(10) {
        async_io::block_on(async_io::Timer::after(Duration::from_millis(100)));
    }
    let out = during();
    w.file("log/stop", "");
    (out, stream.join().unwrap().stdout)
}

/// No disk image can be attached in the matter from inside the jail. The same image attaches
/// outside it; inside, `hdiutil attach` fails because the sandbox refuses its look-up of the
/// disk images helper (the kernel's Sandbox line says so), and nothing is mounted. (Other
/// mounts fail too, but for reasons of their own, so they are no evidence: `mount_smbfs` with
/// no server, `mount -t apfs` of a device that doesn't exist.)
#[test]
fn no_disk_image_can_be_attached_from_inside_the_jail() {
    let w = World::new();
    let m1 = w.at("m1");
    let image = m1.join("x.dmg");
    let image = image.to_str().unwrap();
    let made = [
        "create",
        "-quiet",
        "-size",
        "1m",
        "-fs",
        "HFS+",
        "-volname",
        "cgw-mount",
        image,
    ];
    run_outside("/usr/bin/hdiutil", &made, &w.at("")).assert_ok();
    // Outside the jail, it attaches.
    let point = dir_at(&w.at("outside-mount"));
    let point = point.to_str().unwrap();
    let attach = [
        "attach",
        "-nobrowse",
        "-noverify",
        "-mountpoint",
        point,
        image,
    ];
    run_outside("/usr/bin/hdiutil", &attach, &w.at("")).assert_ok();
    assert_eq!(mounts_below(Path::new(point)).len(), 1, "attached outside");
    run_outside(
        "/usr/bin/hdiutil",
        &["detach", "-force", point],
        Path::new("/"),
    )
    .assert_ok();
    // Inside, the sandbox refuses it.
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let script = "echo $$; exec /usr/bin/hdiutil attach -nobrowse -noverify x.dmg";
    let (out, log) = sandbox_lines(&w, || sh(&launcher, &m1, script, &[]));
    let mounted = mounts_below(&m1);
    for line in &mounted {
        if let Some(at) = line.split(" on ").nth(1).and_then(|r| r.split(" (").next()) {
            drop(run_outside(
                "/usr/bin/hdiutil",
                &["detach", "-force", at],
                Path::new("/"),
            ));
        }
    }
    assert_eq!(
        mounted,
        Vec::<String>::new(),
        "attached in the jail: {out:#?}"
    );
    assert_ne!(out.code, Some(0), "{out:#?}");
    let pid = out
        .stdout
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let want = format!("hdiutil({pid}) deny(1) mach-lookup com.apple.system.hdiejectd");
    assert!(
        log.contains(&want),
        "no Sandbox refusal {want:?}: {out:#?}\n{log}"
    );
}
