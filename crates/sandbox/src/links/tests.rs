use super::*;
use crate::test_support::TempDir;

fn canon(p: &Path) -> PathBuf {
    dunce::canonicalize(p).unwrap()
}

#[test]
fn a_missing_matter_fails_the_scan_and_gives_no_links() {
    let dir = TempDir::new("links-missing");
    let gone = dir.path().join("gone");
    assert!(matches!(scan_matter(&gone), Err(Error::Probe(_))));
    assert_eq!(scan_links(&gone), Vec::new());
}

#[test]
fn a_matter_of_plain_files_has_no_findings() {
    let dir = TempDir::new("links-plain");
    dir.file("matter/a.docx", "a");
    dir.file("matter/sub/b.pdf", "b");
    let scan = scan_matter(&dir.path().join("matter")).unwrap();
    assert_eq!(scan, MatterScan::default());
}

#[cfg(unix)]
#[test]
fn symlinks_inside_pass_and_outside_or_broken_are_found() {
    let dir = TempDir::new("links-sym");
    let inner = dir.file("matter/real.txt", "in");
    let secret = dir.file("outside/secret.txt", "s");
    dir.dir("outside/folder");
    dir.symlink("matter/in.txt", &inner);
    dir.symlink("matter/out.txt", &secret);
    dir.symlink("matter/out-folder", &dir.path().join("outside/folder"));
    dir.symlink("matter/broken.txt", Path::new("../nowhere/x.txt"));
    let matter = canon(&dir.path().join("matter"));

    let mut found = scan_links(&dir.path().join("matter"));
    found.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(
        found,
        [
            LinkFinding {
                path: matter.join("broken.txt"),
                kind: LinkKind::Symlink,
                target: Some(matter.join("../nowhere/x.txt")),
            },
            LinkFinding {
                path: matter.join("out-folder"),
                kind: LinkKind::Symlink,
                target: Some(canon(&dir.path().join("outside/folder"))),
            },
            LinkFinding {
                path: matter.join("out.txt"),
                kind: LinkKind::Symlink,
                target: Some(canon(&secret)),
            },
        ]
    );
}

#[cfg(unix)]
#[test]
fn hard_links_are_found_without_a_target() {
    let dir = TempDir::new("links-hard");
    let secret = dir.file("outside/secret.txt", "s");
    dir.dir("matter");
    dir.hard_link("matter/copy.txt", &secret);
    let found = scan_links(&dir.path().join("matter"));
    assert_eq!(
        found,
        [LinkFinding {
            path: canon(&dir.path().join("matter")).join("copy.txt"),
            kind: LinkKind::HardLink,
            target: None,
        }]
    );
}

#[cfg(unix)]
#[test]
fn a_git_folder_is_walked_for_hard_links_only() {
    let dir = TempDir::new("links-git");
    let secret = dir.file("outside/secret.txt", "s");
    dir.dir("matter/deep/.git/objects");
    dir.hard_link("matter/deep/.git/objects/pack", &secret);
    // Inside a .git: a symbolic link out and a nested .git name give nothing.
    dir.symlink("matter/deep/.git/out", &secret);
    dir.dir("matter/deep/.git/modules/m/.git");
    let scan = scan_matter(&dir.path().join("matter")).unwrap();
    let matter = canon(&dir.path().join("matter"));
    assert_eq!(
        scan.links(),
        [LinkFinding {
            path: matter.join("deep/.git/objects/pack"),
            kind: LinkKind::HardLink,
            target: None,
        }]
    );
    assert_eq!(scan.git(), [matter.join("deep/.git")]);
}

#[cfg(unix)]
#[test]
fn a_file_whose_every_name_is_in_the_matter_is_not_a_finding() {
    let dir = TempDir::new("links-inside");
    let original = dir.file("matter/a.txt", "a");
    dir.dir("matter/sub/.git");
    dir.hard_link("matter/sub/b.txt", &original);
    dir.hard_link("matter/sub/.git/c", &original);
    // A file with two names in the matter and a third outside: every name in the matter is
    // a finding.
    let shared = dir.file("matter/d.txt", "d");
    dir.hard_link("matter/sub/e.txt", &shared);
    dir.hard_link("outside-e.txt", &shared);
    let scan = scan_matter(&dir.path().join("matter")).unwrap();
    let matter = canon(&dir.path().join("matter"));
    let found: Vec<&Path> = scan.links().iter().map(|f| f.path.as_path()).collect();
    assert_eq!(found, [matter.join("d.txt"), matter.join("sub/e.txt")]);
    assert_eq!(
        scan.linked,
        [vec![
            matter.join("a.txt"),
            matter.join("sub/.git/c"),
            matter.join("sub/b.txt"),
        ]]
    );
}

#[test]
fn a_git_file_names_its_folder_inside_the_matter_only() {
    let dir = TempDir::new("links-gitfile");
    dir.dir("matter/.bare/worktrees/a");
    dir.file("matter/a/.git", "gitdir: ../.bare/worktrees/a\n");
    dir.file("matter/b/.git", "gitdir: ../../outside/repo\n");
    dir.dir("outside/repo");
    dir.file("matter/c/.git", "gitdir: ..\n");
    let scan = scan_matter(&dir.path().join("matter")).unwrap();
    let matter = canon(&dir.path().join("matter"));
    assert_eq!(
        scan.git(),
        [
            matter.join("a/.git"),
            matter.join("b/.git"),
            matter.join("c/.git")
        ]
    );
    assert_eq!(scan.git_dirs(), [matter.join(".bare/worktrees/a")]);
}

#[cfg(unix)]
#[test]
fn a_git_link_to_a_folder_in_the_matter_names_that_folder() {
    let dir = TempDir::new("links-gitlink");
    dir.dir("matter/.realgit");
    dir.symlink("matter/.git", Path::new(".realgit"));
    let scan = scan_matter(&dir.path().join("matter")).unwrap();
    let matter = canon(&dir.path().join("matter"));
    assert_eq!(scan.links().len(), 0);
    assert_eq!(scan.git(), [matter.join(".git")]);
    assert_eq!(scan.git_dirs(), [matter.join(".realgit")]);
}

/// The walk on a generated 50,000-file matter: 50 folders of 1,000 files, some nested. Run by
/// hand with `cargo nextest run -p clauseo-sandbox --run-ignored only walk_of` and read the
/// time it prints. Not in CI: it writes 50,000 files.
#[test]
#[ignore = "writes 50,000 files; run by hand to measure the walk"]
#[expect(
    clippy::disallowed_methods,
    reason = "a by-hand measurement of the walk's wall time; never run in CI"
)]
fn walk_of_fifty_thousand_files() {
    let dir = TempDir::new("links-bench");
    for folder in 0..50 {
        let rel = format!("matter/f{folder}/n{}", folder % 5);
        dir.dir(&rel);
        for file in 0..1_000 {
            dir.file(&format!("{rel}/doc{file}.txt"), "x");
        }
    }
    let matter = dir.path().join("matter");
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = std::time::Instant::now();
        let scan = scan_matter(&matter).unwrap();
        times.push(start.elapsed());
        assert_eq!(scan.links().len(), 0);
    }
    times.sort();
    println!(
        "walk of 50,000 files: runs {times:?}, median {:?}",
        times[2]
    );
}
