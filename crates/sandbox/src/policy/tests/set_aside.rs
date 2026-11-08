//! The git watch's set-aside folder: inside a hidden path, apart from every path the agent
//! reaches, and denied last by the Seatbelt profile.

use super::*;

#[test]
fn the_set_aside_folder_is_kept_canonical_and_need_not_exist() {
    let w = World::new("set-aside-kept");
    let policy = w
        .builder()
        .hidden(w.at("data"))
        .git_set_aside(w.at("data/sandbox/git-set-aside"))
        .build()
        .unwrap();
    assert_eq!(
        policy.git_set_aside(),
        Some(canon(&w.at("data")).join("sandbox/git-set-aside").as_path())
    );
    assert_eq!(w.builder().build().unwrap().git_set_aside(), None);
}

#[test]
fn a_set_aside_folder_outside_the_hidden_paths_or_meeting_a_reach_is_refused() {
    let w = World::new("set-aside-refused");
    let outside = w.at("other/set-aside");
    let hidden = |b: PolicyBuilder| b.hidden(w.at("data")).hidden(w.at("other"));
    refused(
        w.builder()
            .hidden(w.at("data"))
            .git_set_aside(&outside)
            .build(),
        "must be inside a hidden path",
        &outside,
    );
    let notes = w.at("data/agent-state/m1/notes");
    let reach = "may not hold or sit inside a path the agent reaches";
    // Holding the notes folder, or inside it.
    let holds = w.at("data/agent-state");
    refused(
        hidden(w.builder())
            .notes(&notes)
            .git_set_aside(&holds)
            .build(),
        reach,
        &holds,
    );
    let inside = notes.join("aside");
    refused(
        hidden(w.builder())
            .notes(&notes)
            .git_set_aside(&inside)
            .build(),
        reach,
        &inside,
    );
    // Inside a runtime path allowed again in the data dir.
    w.dir.dir("data/runtime");
    let in_runtime = w.at("data/runtime/aside");
    refused(
        hidden(w.builder())
            .runtime(w.at("data/runtime"))
            .git_set_aside(&in_runtime)
            .build(),
        reach,
        &in_runtime,
    );
}

#[test]
fn the_profile_denies_the_set_aside_folder_last() {
    let w = World::new("set-aside-profile");
    let policy = w
        .builder()
        .hidden(w.at("data"))
        .notes(w.at("data/agent-state/m1/notes"))
        .git_set_aside(w.at("data/sandbox/git-set-aside"))
        .build()
        .unwrap();
    let profile = crate::SeatbeltProfile::new(&policy, "clauseo-t-0").unwrap();
    let text = profile.text();
    let rule = "(deny file-read* file-write*\n  (subpath (param \"GIT_SET_ASIDE\"))";
    let at = text.find(rule).unwrap_or_else(|| panic!("{text}"));
    let last_allow = text.rfind("(allow file-").unwrap();
    assert!(at > last_allow, "an allow comes after the deny:\n{text}");
    assert!(
        profile
            .params()
            .iter()
            .any(|(name, path)| name == "GIT_SET_ASIDE" && path.ends_with("git-set-aside"))
    );
}
