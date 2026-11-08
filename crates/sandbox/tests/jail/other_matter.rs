//! Reads of another matter (R18) under the real `sandbox-exec`: the rule order holds. The read
//! grant comes after the hidden paths' deny and wins over it; the deny of `.git`, `.clauseo`,
//! `.claude`, `.codex` and `.agents` comes after the grant and wins over it, at any depth, in
//! any letter case, and through a link; nothing in the grant is written.

use crate::support::{World, launcher, sh};

/// Reads each path given and prints `READ <path>` or `REFUSED <path>`.
const READ_EACH: &str = r#"for f in "$@"; do if cat "$f" >/dev/null 2>&1; then echo "READ $f"; else echo "REFUSED $f"; fi; done; exit 0"#;

/// Appends to each path given and prints `WROTE <path>` when it could.
const APPEND_EACH: &str =
    r#"for f in "$@"; do printf x >> "$f" 2>/dev/null && echo "WROTE $f"; done; exit 0"#;

/// `m2` with a document, its `.git` and `.clauseo`, a nested `.GIT`, its agents' folders, a
/// look-alike `.github`, and a link in it that leads into its `.git`.
fn other_matter(w: &World) {
    w.file("m2/doc.txt", "doc\n");
    w.file("m2/sub/note.txt", "note\n");
    w.file("m2/.git/config", "[core]\n");
    w.file("m2/.clauseo/x", "data\n");
    w.file("m2/sub/.GIT/HEAD", "ref\n");
    w.file("m2/.claude/settings.json", "{}\n");
    w.file("m2/sub/.codex/config.toml", "x\n");
    w.file("m2/.agents/a.md", "a\n");
    w.file("m2/.github/ci.yml", "ci\n");
    w.symlink("m2/.git/config", "m2/sub/git-config");
}

fn lines(stdout: &str, word: &str) -> Vec<String> {
    let mut out: Vec<String> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix(word))
        .map(|rest| rest.trim().to_owned())
        .collect();
    out.sort();
    out
}

#[test]
fn a_read_grant_of_another_matter_reads_it_but_not_its_git_or_data_folder() {
    let w = World::new();
    other_matter(&w);
    let policy = w
        .builder()
        .hidden_matter(&w.at("m2"), "m2-id")
        .other_matter_read(&w.at("m2"), false)
        .build()
        .unwrap();
    let (launcher, _) = launcher(policy);
    let at = |rel: &str| w.at(rel).to_str().unwrap().to_owned();
    let readable = ["m2/doc.txt", "m2/sub/note.txt", "m2/.github/ci.yml"].map(at);
    let refused = [
        "m2/.git/config",
        "m2/.clauseo/x",
        "m2/sub/.GIT/HEAD",
        "m2/.claude/settings.json",
        "m2/sub/.codex/config.toml",
        "m2/.agents/a.md",
        "m2/sub/git-config",
    ]
    .map(at);
    let mut all: Vec<&str> = readable.iter().map(String::as_str).collect();
    all.extend(refused.iter().map(String::as_str));
    let out = sh(&launcher, &w.at("m1"), READ_EACH, &all);
    out.assert_ok();
    let mut want_read = readable.to_vec();
    want_read.sort();
    let mut want_refused = refused.to_vec();
    want_refused.sort();
    assert_eq!(lines(&out.stdout, "READ "), want_read, "{out:#?}");
    assert_eq!(lines(&out.stdout, "REFUSED "), want_refused, "{out:#?}");

    // The folder lists; nothing in it is written, made or removed.
    let out = sh(&launcher, &w.at("m1"), r#"ls "$1""#, &[&at("m2")]);
    out.assert_ok();
    assert!(out.stdout.contains("doc.txt"), "{out:#?}");
    let out = sh(
        &launcher,
        &w.at("m1"),
        APPEND_EACH,
        &[&at("m2/doc.txt"), &at("m2/new.txt")],
    );
    out.assert_ok();
    assert!(!out.stdout.contains("WROTE"), "{out:#?}");
    let out = sh(&launcher, &w.at("m1"), r#"rm "$1""#, &[&at("m2/doc.txt")]);
    assert_ne!(out.code, Some(0), "{out:#?}");
    assert_eq!(
        std::fs::read_to_string(w.at("m2/doc.txt")).unwrap(),
        "doc\n"
    );
    assert!(!w.at("m2/new.txt").exists());
}

#[test]
fn a_read_grant_of_one_file_reads_that_file_only() {
    let w = World::new();
    other_matter(&w);
    let policy = w
        .builder()
        .hidden_matter(&w.at("m2"), "m2-id")
        .other_matter_read(&w.at("m2/doc.txt"), true)
        .build()
        .unwrap();
    let (launcher, _) = launcher(policy);
    let at = |rel: &str| w.at(rel).to_str().unwrap().to_owned();
    let (doc, note, brief) = (at("m2/doc.txt"), at("m2/sub/note.txt"), at("m2/brief.txt"));
    let out = sh(&launcher, &w.at("m1"), READ_EACH, &[&doc, &note, &brief]);
    out.assert_ok();
    assert_eq!(
        lines(&out.stdout, "READ "),
        std::slice::from_ref(&doc),
        "{out:#?}"
    );
    let out = sh(&launcher, &w.at("m1"), r#"ls "$1""#, &[&at("m2")]);
    assert_ne!(
        out.code,
        Some(0),
        "the matter's folder doesn't list: {out:#?}"
    );
    let out = sh(&launcher, &w.at("m1"), APPEND_EACH, &[&doc]);
    assert!(!out.stdout.contains("WROTE"), "{out:#?}");
}

#[test]
fn a_read_grant_of_a_subfolder_refuses_links_out_of_it() {
    let w = World::new();
    other_matter(&w);
    // Links in the granted subfolder to the matter's own file outside it, to its data folder
    // and out of the matter; Seatbelt checks the path each resolves to.
    w.symlink("m2/doc.txt", "m2/sub/to-doc");
    w.symlink("m2/.clauseo/x", "m2/sub/to-data");
    w.symlink("outside/secret.txt", "m2/sub/to-outside");
    let policy = w
        .builder()
        .hidden_matter(&w.at("m2"), "m2-id")
        .other_matter_read(&w.at("m2/sub"), false)
        .build()
        .unwrap();
    let (launcher, _) = launcher(policy);
    let at = |rel: &str| w.at(rel).to_str().unwrap().to_owned();
    let refused = [
        "m2/doc.txt",
        "m2/.github/ci.yml",
        "m2/sub/.GIT/HEAD",
        "m2/sub/.codex/config.toml",
        "m2/sub/git-config",
        "m2/sub/to-doc",
        "m2/sub/to-data",
        "m2/sub/to-outside",
    ]
    .map(at);
    let note = at("m2/sub/note.txt");
    let mut all = vec![note.as_str()];
    all.extend(refused.iter().map(String::as_str));
    let out = sh(&launcher, &w.at("m1"), READ_EACH, &all);
    out.assert_ok();
    assert_eq!(lines(&out.stdout, "READ "), [note], "{out:#?}");
    let mut want_refused = refused.to_vec();
    want_refused.sort();
    assert_eq!(lines(&out.stdout, "REFUSED "), want_refused, "{out:#?}");
    // The subfolder lists; the matter's folder above it doesn't.
    let out = sh(&launcher, &w.at("m1"), r#"ls "$1""#, &[&at("m2/sub")]);
    out.assert_ok();
    assert!(out.stdout.contains("note.txt"), "{out:#?}");
    let out = sh(&launcher, &w.at("m1"), r#"ls "$1""#, &[&at("m2")]);
    assert_ne!(out.code, Some(0), "{out:#?}");
}

#[test]
fn a_git_or_clauseo_made_during_the_run_in_a_granted_folder_stays_refused() {
    let w = World::new();
    other_matter(&w);
    w.file("m2/.git/worktrees/made/HEAD", "ref\n");
    let policy = w
        .builder()
        .hidden_matter(&w.at("m2"), "m2-id")
        .other_matter_read(&w.at("m2/sub"), false)
        .build()
        .unwrap();
    let (launcher, _) = launcher(policy);
    // Made after the profile is written: a worktree's `.git` file and a `.clauseo` file, and a
    // `.git` folder a level down; each is refused by name, as is the `gitdir:` target.
    w.file("m2/sub/made/.git", "gitdir: ../../.git/worktrees/made\n");
    w.file("m2/sub/made/.clauseo", "data\n");
    w.file("m2/sub/made/deep/.git/HEAD", "ref\n");
    w.file("m2/sub/made/plain.txt", "plain\n");
    let at = |rel: &str| w.at(rel).to_str().unwrap().to_owned();
    let refused = [
        "m2/sub/made/.git",
        "m2/sub/made/.clauseo",
        "m2/sub/made/deep/.git/HEAD",
        "m2/.git/worktrees/made/HEAD",
    ]
    .map(at);
    let plain = at("m2/sub/made/plain.txt");
    let mut all = vec![plain.as_str()];
    all.extend(refused.iter().map(String::as_str));
    let out = sh(&launcher, &w.at("m1"), READ_EACH, &all);
    out.assert_ok();
    assert_eq!(lines(&out.stdout, "READ "), [plain], "{out:#?}");
    let mut want_refused = refused.to_vec();
    want_refused.sort();
    assert_eq!(lines(&out.stdout, "REFUSED "), want_refused, "{out:#?}");
}

/// R18 MEDIUM-1: APFS opens `.clauſeo` (a long s, U+017F) as `.clauseo`, and `.agentſ` as
/// `.agents`; each is refused through a grant on their parent, as are other letter cases.
/// Seatbelt matches the name as stored, so the ASCII-only regex refused these too (measured);
/// the profile's refused names take every spelling that folds to their letters all the same.
#[test]
fn refused_names_spelled_in_any_case_or_with_a_long_s_stay_refused() {
    let w = World::new();
    other_matter(&w);
    w.file("m2/.agents/x", "a\n");
    w.file("m2/.git/HEAD", "ref\n");
    let policy = w
        .builder()
        .hidden_matter(&w.at("m2"), "m2-id")
        .other_matter_read(&w.at("m2"), false)
        .build()
        .unwrap();
    let (launcher, _) = launcher(policy);
    let at = |rel: &str| w.at(rel).to_str().unwrap().to_owned();
    let refused = [
        "m2/.clau\u{17F}eo/x",
        "m2/.CLAUSEO/x",
        "m2/.ClAuSeO/x",
        "m2/.agent\u{17F}/x",
        "m2/.GIT/HEAD",
    ]
    .map(at);
    // On a case-insensitive volume (this Mac's), each spelling reaches the real file outside the
    // jail, so the refusals below are the profile's doing.
    if w.at("m2/.CLAUSEO/x").exists() {
        for path in &refused {
            assert!(std::fs::read(path).is_ok(), "{path}");
        }
    }
    let doc = at("m2/doc.txt");
    let mut all = vec![doc.as_str()];
    all.extend(refused.iter().map(String::as_str));
    let out = sh(&launcher, &w.at("m1"), READ_EACH, &all);
    out.assert_ok();
    assert_eq!(lines(&out.stdout, "READ "), [doc], "{out:#?}");
    let mut want_refused = refused.to_vec();
    want_refused.sort();
    assert_eq!(lines(&out.stdout, "REFUSED "), want_refused, "{out:#?}");
}
