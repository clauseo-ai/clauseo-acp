//! Where a browser binary could be planted for the guard to start: `/Applications`,
//! `~/Applications` and the data folder's `browsers/`, which the host's runtime grant makes
//! readable and runnable in the jail. Under the tier-B profile every write there is refused:
//! creating a file, making a folder, renaming a matter file in, a hard link, a clone and a
//! symbolic link, and opening an existing file for writing (which writes nothing, so a failure
//! of the jail changes no app). Swapping a matter file with an existing one (`RENAME_SWAP`) is
//! tried only on the test's own folders: on a real app a failure of the jail would move its file.

use std::path::{Path, PathBuf};

use crate::support::{Output, World, launcher, run_outside, sh};

/// Tries each write on the folder `$1`, with the matter file `$2` to rename, link and clone in,
/// and the existing file `$3` (or empty) to open for writing and, when `$4` is `swap`, to swap
/// with `$2` (`renameatx_np`, syscall 488, `AT_FDCWD` -2, `RENAME_SWAP` 2). Prints
/// `<step>: <error>` for each refusal and `<step> DONE` for each write that went through.
const WRITES: &str = r#"
    printf x > "$1/clauseo-jail-probe" && echo "create DONE"
    mkdir "$1/clauseo-jail-probe-dir" && echo "mkdir DONE"
    /usr/bin/perl -e 'rename($ARGV[0], $ARGV[1]) or die "rename: $!\n"; print "rename DONE\n"' \
        "$2" "$1/clauseo-jail-probe-moved"
    ln "$2" "$1/clauseo-jail-probe-link" && echo "link DONE"
    cp -c "$2" "$1/clauseo-jail-probe-clone" && echo "clone DONE"
    ln -s /bin/sh "$1/clauseo-jail-probe-symlink" && echo "symlink DONE"
    if [ "$4" = swap ]; then
        /usr/bin/perl -e 'syscall(488, -2, $ARGV[0], -2, $ARGV[1], 2) == 0 or die "swap: $!\n";
            print "swap DONE\n"' "$2" "$3"
    fi
    if [ -n "$3" ]; then exec 3<>"$3" && echo "overwrite DONE"; fi
    exit 0
"#;

/// The names [`WRITES`] makes in a folder.
const MADE: [&str; 6] = [
    "clauseo-jail-probe",
    "clauseo-jail-probe-dir",
    "clauseo-jail-probe-moved",
    "clauseo-jail-probe-link",
    "clauseo-jail-probe-clone",
    "clauseo-jail-probe-symlink",
];

/// Whether [`assert_writes_refused`] also tries the swap.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Swap {
    Try,
    Skip,
}

/// Runs [`WRITES`] on `folder` in the jail and asserts every step was refused with EPERM and
/// nothing appeared.
#[track_caller]
fn assert_writes_refused(
    launcher: &dyn clauseo_process::Launcher,
    w: &World,
    folder: &Path,
    existing: Option<&Path>,
    swap: Swap,
) -> Output {
    let source = w.file("m1/to-move.txt", "matter file\n");
    let existing = existing.map_or(String::new(), |p| p.to_str().unwrap().to_owned());
    let swap = swap == Swap::Try && !existing.is_empty();
    let args = [
        folder.to_str().unwrap(),
        source.to_str().unwrap(),
        &existing,
        if swap { "swap" } else { "" },
    ];
    let out = sh(launcher, &w.at("m1"), WRITES, &args);
    assert!(
        !out.stdout.contains("DONE"),
        "a write went through: {out:#?}"
    );
    let mut steps = vec!["create", "mkdir", "rename", "link", "clone", "symlink"];
    if swap {
        steps.push("swap");
    }
    if !existing.is_empty() {
        steps.push("overwrite");
    }
    let refusals = out.stderr.matches("Operation not permitted").count();
    assert_eq!(
        refusals,
        steps.len(),
        "expected EPERM for {steps:?}: {out:#?}"
    );
    assert!(
        out.stderr.contains("rename: Operation not permitted"),
        "{out:#?}"
    );
    for name in MADE {
        assert!(
            !folder.join(name).exists(),
            "{} appeared",
            folder.join(name).display()
        );
    }
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        "matter file\n",
        "the matter file moved or was swapped"
    );
    out
}

/// A file in `folder` (at most two levels down) the user may write outside the jail, to open
/// for writing in it.
fn writable_file_in(folder: &Path) -> Option<PathBuf> {
    let script =
        r#"find "$1" -maxdepth 3 -type f -perm -u+w -user "$(id -u)" 2>/dev/null | head -1"#;
    let out = run_outside(
        "/bin/sh",
        &["-c", script, "sh", folder.to_str().unwrap()],
        folder,
    );
    let found = out.stdout.trim();
    (!found.is_empty()).then(|| PathBuf::from(found))
}

#[test]
fn nothing_can_be_planted_in_applications() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let applications = Path::new("/Applications");
    // An existing app's file this user may write outside the jail: without one the overwrite
    // step would prove nothing, so the test fails rather than skip it.
    let existing = writable_file_in(applications)
        .expect("no file in /Applications this user may write, for the overwrite step");
    assert_writes_refused(&launcher, &w, applications, Some(&existing), Swap::Skip);
}

#[test]
fn nothing_can_be_planted_in_the_home_applications_folder() {
    let w = World::new();
    w.file(
        "home/Applications/Tool.app/Contents/Info.plist",
        "<plist/>\n",
    );
    let (launcher, _) = launcher(w.builder().build().unwrap());
    // The policy's home (the profile names no home path but its text-encoding file).
    let fake = w.at("home/Applications");
    let plist = fake.join("Tool.app/Contents/Info.plist");
    assert_writes_refused(&launcher, &w, &fake, Some(&plist), Swap::Try);
    assert_eq!(std::fs::read_to_string(&plist).unwrap(), "<plist/>\n");

    // The real one: its folder when it exists, else making it is refused.
    let home = dunce::canonicalize(std::env::var("HOME").unwrap()).unwrap();
    let real = home.join("Applications");
    if real.is_dir() {
        let existing = writable_file_in(&real);
        assert_writes_refused(&launcher, &w, &real, existing.as_deref(), Swap::Skip);
    } else {
        let out = sh(
            &launcher,
            &w.at("m1"),
            r#"mkdir "$1""#,
            &[real.to_str().unwrap()],
        );
        out.assert_refused();
        assert!(!real.exists(), "{} was made", real.display());
    }
}

#[test]
fn nothing_can_be_planted_in_the_data_folders_browsers() {
    let w = World::new();
    // The host's layout: `<data>/browsers/<version>/...`, granted as a runtime (read and run).
    let binary = "data/browsers/chrome-1.0/chrome-headless-shell";
    w.file(binary, "#!/bin/sh\necho stand-in\n");
    let browsers = w.at("data/browsers");
    let policy = w.builder().runtime(&browsers).build().unwrap();
    let (launcher, _) = launcher(policy);
    // The grant works: the jail reads the stand-in.
    let read = sh(
        &launcher,
        &w.at("m1"),
        r#"cat "$1""#,
        &[w.at(binary).to_str().unwrap()],
    );
    assert!(read.assert_ok().stdout.contains("stand-in"), "{read:#?}");
    assert_writes_refused(&launcher, &w, &browsers, Some(&w.at(binary)), Swap::Try);
    let version = w.at("data/browsers/chrome-1.0");
    assert_writes_refused(&launcher, &w, &version, Some(&w.at(binary)), Swap::Try);
    let content = std::fs::read_to_string(w.at(binary)).unwrap();
    assert_eq!(content, "#!/bin/sh\necho stand-in\n");
}
