//! The lawyer-side git watch with a real jail (decision record 0038): no repository is made in
//! the session's temp folder or notes; the first find ends every process of the session, one
//! that left the group with `setsid` included, before anything moves; and a matter on another
//! volume sets aside on that volume, in a `.clauseo` folder the agent can't reach; and nothing
//! git reads is made at a launch repository's `commondir` target.

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
    // A link out of the matter ends the session and moves aside on the matter's own volume.
    sh(
        &launcher,
        &m1,
        r#"ln -s "$1" out && exec /bin/sleep 30"#,
        &[&state],
    );
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
