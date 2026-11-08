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

/// A runtime inside a place the agent writes would fail to start with
/// `Policy::native_code_deny` on (nothing there maps as code), so the build refuses it, naming
/// the runtime and the folder, whether the switch is on or off. (A runtime equal to the matter
/// gets the older refusal, above.)
#[test]
fn a_runtime_inside_a_writable_folder_is_refused() {
    let w = World::new("runtime-writable");
    for dir in [
        "matter/node",
        "state/bin",
        "data/agent-state/m1/notes/tool",
        "data/run/s1/tmp/py",
        "home/.claude/.storage-write.lock",
    ] {
        w.dir.dir(dir);
    }
    let notes = w.at("data/agent-state/m1/notes");
    let tmp = "data/run/s1/tmp";
    let cases = [
        (w.builder(), "matter/node", "matter", "the matter"),
        (w.builder(), "state/bin", "state", "the state folder"),
        (
            w.builder().notes(&notes),
            "data/agent-state/m1/notes/tool",
            "data/agent-state/m1/notes",
            "the notes folder",
        ),
        (
            w.builder().tmpdir(w.at(tmp)),
            "data/run/s1/tmp/py",
            tmp,
            "the session temp folder",
        ),
        (
            w.builder().claude_store(w.at("home/.claude")),
            "home/.claude/.storage-write.lock",
            "home/.claude",
            "the Claude store",
        ),
    ];
    for (builder, runtime, folder, what) in cases {
        for on in [false, true] {
            let built = builder
                .clone()
                .native_code_deny(on)
                .runtime(w.at(runtime))
                .build();
            let Err(Error::Policy(message)) = built else {
                panic!("{runtime} (switch {on}) was not refused: {built:?}");
            };
            assert!(message.contains(what), "{message}");
            let runtime = canon(&w.at(runtime)).display().to_string();
            assert!(
                message.contains(&format!(": {runtime} is in ")),
                "{message}"
            );
            let folder = canon(&w.at(folder)).display().to_string();
            assert!(message.ends_with(&format!(" is in {folder}")), "{message}");
        }
    }
    let outside = w.dir.dir("runtime/node");
    let policy = w.builder().native_code_deny(true).runtime(&outside).build();
    assert!(policy.is_ok(), "{policy:?}");
}
