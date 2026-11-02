//! The paths allowed again after the hidden paths, and the folders an agent could replace:
//! review round 1, items 1, 4, 8 and 13.

use super::*;

#[cfg(unix)]
#[test]
fn the_matter_state_and_notes_may_not_be_symbolic_links() {
    let w = World::new("root-links");
    w.dir.dir("real-matter");
    w.dir.dir("real-state");
    w.dir.dir("data/agent-state/m1/real-notes");
    let matter = w.dir.symlink("linked-matter", &w.at("real-matter"));
    let state = w.dir.symlink("linked-state", &w.at("real-state"));
    let notes = w
        .dir
        .symlink("data/agent-state/m1/linked-notes", &w.at("other"));
    let rule = "may not be a symbolic link";
    refused(
        Policy::builder(&matter, w.at("state"))
            .home(w.at("home"))
            .build(),
        rule,
        &matter,
    );
    refused(
        Policy::builder(w.at("matter"), &state)
            .home(w.at("home"))
            .build(),
        rule,
        &state,
    );
    refused(w.builder().notes(&notes).build(), rule, &notes);
    // A link above the folder is fine: only its own name is checked.
    let above = w.dir.symlink("above", w.dir.path());
    w.builder()
        .notes(above.join("data/agent-state/m1/notes"))
        .build()
        .unwrap();
}

#[test]
fn the_state_and_notes_folders_may_not_hold_a_hidden_path_or_the_matter() {
    let w = World::new("state-notes-hold");
    w.dir.dir("data/agent-state/m2/notes");
    refused(
        w.builder().hidden(w.at("state")).build(),
        "the state folder may not equal or hold a hidden path",
        &w.at("state"),
    );
    refused(
        w.builder().hidden(w.at("state/inner")).build(),
        "the state folder may not equal or hold a hidden path",
        &w.at("state"),
    );
    // Notes holding another matter's notes, and notes holding this matter.
    refused(
        w.builder()
            .hidden(w.at("data/agent-state/m2"))
            .notes(w.at("data/agent-state"))
            .build(),
        "the notes folder may not equal or hold a hidden path",
        &w.at("data/agent-state"),
    );
    refused(
        w.builder().notes(w.dir.path()).build(),
        "the notes folder may not be inside the matter or hold it",
        w.dir.path(),
    );
    // Notes inside the hidden data dir, R17's layout, is fine.
    w.builder()
        .hidden(w.at("data"))
        .notes(w.at("data/agent-state/m1/notes"))
        .build()
        .unwrap();
}

#[test]
fn runtime_and_read_paths_may_not_hold_what_they_would_undo() {
    let w = World::new("reallowed");
    w.dir.dir("data/agents/codex");
    let hidden = || w.builder().hidden(w.at("data"));
    refused(
        hidden().runtime(w.root()).build(),
        "a runtime path may not be the root folder",
        &w.root(),
    );
    refused(
        hidden().read(w.at("home")).build(),
        "a read path may not equal or hold the home folder",
        &w.at("home"),
    );
    // The review's probe J: a read path holding the matter and a hidden path.
    refused(
        hidden().read(w.dir.path()).build(),
        "a read path may not equal or hold the home folder",
        w.dir.path(),
    );
    w.dir.dir("matter/sub");
    refused(
        hidden().runtime(w.at("matter")).build(),
        "a runtime path may not equal or hold the matter",
        &w.at("matter"),
    );
    refused(
        hidden().runtime(w.at("data")).build(),
        "a runtime path may not equal or hold a hidden path",
        &w.at("data"),
    );
    refused(
        w.builder()
            .hidden(w.at("data/agents/codex/secret"))
            .read(w.at("data/agents"))
            .build(),
        "a read path may not equal or hold a hidden path",
        &w.at("data/agents"),
    );
    // Inside a hidden path, inside the home folder or inside the matter: fine.
    w.dir.dir("home/skills");
    hidden()
        .runtime(w.at("data/agents/codex"))
        .read(w.at("home/skills"))
        .build()
        .unwrap();
    hidden()
        .runtime(w.at("data/agents/codex"))
        .read(w.at("matter/sub"))
        .build()
        .unwrap();
}

#[test]
fn the_credential_link_must_be_inside_the_state_folder() {
    let w = World::new("cred-inside");
    let outside = w.dir.file("other/secret.txt", "s");
    // The review's probe B: a "link" that is the outside file itself.
    refused(
        w.builder().credential_link(&outside).build(),
        "the credential link must be inside the state folder",
        &outside,
    );
    refused(
        w.builder().credential_link(w.at("state")).build(),
        "the credential link must be inside the state folder",
        &w.at("state"),
    );
    let policy = w
        .builder()
        .credential_link(w.at("state/auth.json"))
        .build()
        .unwrap();
    assert_eq!(
        policy.credential_link(),
        Some(canon(&w.at("state")).join("auth.json").as_path())
    );
}

#[cfg(target_os = "macos")]
#[test]
fn classify_folds_letter_case_on_macos() {
    let w = World::new("classify-case");
    w.dir.file("matter/.git/config", "[core]\n");
    let policy = w.builder().hidden(w.at("data")).build().unwrap();
    let root = canon(w.dir.path());
    assert_eq!(
        policy.classify(&root.join("DATA/x"), Access::Read),
        PathClass::Hidden
    );
    assert_eq!(
        policy.classify(&root.join("matter/.GIT/config"), Access::Write),
        PathClass::ReadOnly
    );
    assert_eq!(
        policy.classify(&root.join("Matter/.Codex/config.toml"), Access::Write),
        PathClass::ReadOnly
    );
    assert_eq!(
        policy.classify(&root.join("matter/other.txt"), Access::Write),
        PathClass::Outside
    );
}

#[test]
fn a_nested_claude_md_and_claude_local_md_are_read_only() {
    let w = World::new("instructions");
    let policy = w.builder().build().unwrap();
    let matter = canon(&w.at("matter"));
    assert!(
        policy
            .read_only_in_matter()
            .contains(&PathRule::Subpath(matter.join("CLAUDE.local.md")))
    );
    for rel in ["a/b/CLAUDE.md", "sub/claude.md", "CLAUDE.md"] {
        assert_eq!(
            policy.classify(&matter.join(rel), Access::Write),
            PathClass::ReadOnly,
            "{rel}"
        );
    }
    assert_eq!(
        policy.classify(&matter.join("sub/CLAUDE.md.bak"), Access::Write),
        PathClass::Outside
    );
}
