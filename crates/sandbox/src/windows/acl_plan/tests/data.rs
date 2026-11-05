//! What the plan refuses around the profile, the sandbox records and the data folder.

use super::*;

/// The plan for `policy`, gated with every folder answering Ok, as an error or not.
fn refusal(w: &World, policy: &Policy) -> Option<(PathKind, FolderRefusal)> {
    let admitted = admitted(w, policy, &[], &FakeFolders::default());
    match plan(&admitted, &sids(), matter_one()) {
        Ok(_) => None,
        Err(Error::FolderUnsupported { path_kind, support }) => Some((path_kind, support)),
        Err(other) => panic!("{other:?}"),
    }
}

#[test]
fn a_notes_folder_on_the_profile_is_refused() {
    // The review's repro: Modify down the tree on the profile.
    let w = World::new("plan-notes-home");
    let policy = w.builder().notes(w.at("home")).build().unwrap();
    let want = (PathKind::Notes, FolderRefusal::HoldsTheProfile);
    assert_eq!(refusal(&w, &policy), Some(want));
}

#[test]
fn a_read_path_over_other_matters_state_is_refused() {
    // The review's repro: `data/agent-state` holds every matter's state and notes.
    let w = World::new("plan-read-agent-state");
    let policy = w.builder().read(w.at("data/agent-state")).build().unwrap();
    let want = (PathKind::Read, FolderRefusal::OverlapsDataFolder);
    assert_eq!(refusal(&w, &policy), Some(want));
}

#[test]
fn the_sandbox_records_folder_is_refused_as_a_runtime_path_or_the_claude_store() {
    let w = World::new("plan-sandbox-records");
    w.dir.dir("data/sandbox");
    let runtime = w.builder().runtime(w.at("data/sandbox")).build().unwrap();
    let want = (PathKind::Runtime, FolderRefusal::OverlapsSandboxRecords);
    assert_eq!(refusal(&w, &runtime), Some(want));
    let store = w
        .builder()
        .claude_store(w.at("data/sandbox"))
        .build()
        .unwrap();
    let got = refusal(&w, &store).unwrap();
    assert_eq!(got.1, FolderRefusal::OverlapsSandboxRecords);
    assert!(
        matches!(got.0, PathKind::ClaudeStore | PathKind::ClaudeStoreFile),
        "{got:?}"
    );
    // The data folder itself as a read path holds the records folder (a policy that doesn't hide
    // it; with it hidden, the policy refuses that path itself).
    let data = Policy::builder(w.at("matter"), w.at(STATE))
        .home(w.at("home"))
        .read(w.at("data"))
        .build()
        .unwrap();
    assert_eq!(
        refusal(&w, &data).unwrap().1,
        FolderRefusal::OverlapsSandboxRecords
    );
}

#[test]
fn a_read_path_in_this_matters_own_state_folder_is_refused() {
    // Read and run grants in the data folder are only for agents' installs.
    let w = World::new("plan-read-own-state");
    w.dir.dir(&format!("{STATE}/skills"));
    let policy = w
        .builder()
        .read(w.at(&format!("{STATE}/skills")))
        .build()
        .unwrap();
    let want = (PathKind::Read, FolderRefusal::OverlapsDataFolder);
    assert_eq!(refusal(&w, &policy), Some(want));
}

#[test]
fn a_runtime_inside_an_agents_install_folder_is_planned_read_only() {
    let w = World::new("plan-agents-runtime");
    w.dir.dir("data/agents/claude-acp/node");
    let policy = w
        .builder()
        .runtime(w.at("data/agents/claude-acp/node"))
        .build()
        .unwrap();
    assert_eq!(refusal(&w, &policy), None);
    let grants = plan(
        &admitted(&w, &policy, &[], &FakeFolders::default()),
        &sids(),
        matter_one(),
    )
    .unwrap();
    let node = w.canonical("data/agents/claude-acp/node");
    let on_node: Vec<&Grant> = grants.grants().iter().filter(|g| g.path == node).collect();
    assert_eq!(on_node.len(), 1, "the pool user alone: {on_node:#?}");
    assert!(on_node.iter().all(|g| g.rights == Rights::ReadAndExecute));
}

#[test]
fn a_writable_target_inside_an_agents_install_folder_is_refused() {
    let w = World::new("plan-agents-writable");
    w.dir.dir("data/agents/claude-acp/node");
    let policy = w
        .builder()
        .notes(w.at("data/agents/claude-acp/node"))
        .build()
        .unwrap();
    let want = (PathKind::Notes, FolderRefusal::OverlapsDataFolder);
    assert_eq!(refusal(&w, &policy), Some(want));
}

#[test]
fn an_agents_home_or_whole_folder_as_a_read_target_is_refused() {
    let w = World::new("plan-agents-home");
    w.dir.dir("data/agents/codex-acp/home/.codex");
    for rel in [
        "data/agents/codex-acp/home",
        "data/agents/codex-acp/home/.codex",
        "data/agents/codex-acp",
    ] {
        let policy = w.builder().read(w.at(rel)).build().unwrap();
        let want = (PathKind::Read, FolderRefusal::OverlapsDataFolder);
        assert_eq!(refusal(&w, &policy), Some(want), "{rel}");
    }
}

#[test]
fn a_claude_store_in_an_agents_home_is_planned() {
    // Exactly the policy's Claude store: its folder and its allowed names, nothing around them.
    let w = World::new("plan-agents-claude-store");
    w.dir.dir("data/agents/claude-acp/home");
    let policy = w
        .builder()
        .claude_store(w.at("data/agents/claude-acp/home"))
        .build()
        .unwrap();
    assert_eq!(refusal(&w, &policy), None);
}

#[test]
fn writable_targets_in_the_data_folder_outside_the_allow_list_are_refused() {
    let w = World::new("plan-allow-list");
    w.dir.dir("data/agents/runtime/node-v22");
    let refused = |policy: Policy, kind: PathKind| {
        let want = (kind, FolderRefusal::OverlapsDataFolder);
        assert_eq!(refusal(&w, &policy), Some(want));
    };
    // The agents folder, and an agent's runtime, as the Claude store.
    let store = PathKind::ClaudeStore;
    refused(
        w.builder()
            .claude_store(w.at("data/agents"))
            .build()
            .unwrap(),
        store,
    );
    let node = w.at("data/agents/runtime/node-v22");
    refused(w.builder().claude_store(node).build().unwrap(), store);
    // The updates folder as the state folder.
    let updates = Policy::builder(w.at("matter"), w.at("data/updates"))
        .home(w.at("home"))
        .hidden(w.at("data"))
        .build()
        .unwrap();
    refused(updates, PathKind::State);
    // Another matter's notes folder as this matter's.
    let other = "data/agent-state/01890a5d-ac96-774b-8cce-b302099a8058/notes";
    refused(
        w.builder().notes(w.at(other)).build().unwrap(),
        PathKind::Notes,
    );
}

#[test]
fn the_real_state_notes_store_and_an_agent_runtime_are_planned() {
    let w = World::new("plan-allow-list-ok");
    w.dir.dir("data/agents/claude-acp/node");
    let policy = w
        .builder()
        .notes(w.at(NOTES))
        .claude_store(w.at(STORE))
        .runtime(w.at("data/agents/claude-acp/node"))
        .build()
        .unwrap();
    assert_eq!(refusal(&w, &policy), None);
    assert_eq!(refusal(&w, &w.full_policy()), None);
    assert!(w.canonical(STATE).ends_with("claude"));
}

#[test]
fn names_windows_reads_as_another_spelling_are_refused() {
    for odd in [
        "/d/agents/claude-acp/home.",
        "/d/agents/claude-acp/home ",
        "/d/agents/claude-acp/home::$INDEX_ALLOCATION",
        "/d/agents/claude-acp/home:stream",
    ] {
        assert!(odd_spelling(Path::new(odd)), "{odd}");
    }
    for fine in [
        "/d/agents/claude-acp/home",
        "/d/notes.d/x",
        "/d/a~b",
        "/d/.config",
    ] {
        assert!(!odd_spelling(Path::new(fine)), "{fine}");
    }
}

// Windows itself drops a trailing dot and opens a stream for `::$INDEX_ALLOCATION`, so these
// folders can only be made, and reach the plan, elsewhere.
#[cfg(not(windows))]
#[test]
fn a_claude_store_spelled_another_way_is_refused() {
    let w = World::new("plan-odd-store");
    for odd in [
        "data/agents/claude-acp/home.",
        "data/agents/claude-acp/home::$INDEX_ALLOCATION",
    ] {
        w.dir.dir(odd);
        let policy = w.builder().claude_store(w.at(odd)).build().unwrap();
        let got = refusal(&w, &policy).unwrap();
        assert_eq!(got.1, FolderRefusal::OddSpelling, "{odd}");
    }
}

#[test]
fn a_matter_named_with_a_tilde_and_digit_is_planned() {
    // `Draft~2` is an ordinary name; the canonical path is what the plan compares.
    let w = World::new("plan-tilde-matter");
    w.dir.dir("Draft~2");
    let policy = Policy::builder(w.at("Draft~2"), w.at(STATE))
        .home(w.at("home"))
        .hidden(w.at("data"))
        .build()
        .unwrap();
    assert_eq!(refusal(&w, &policy), None);
    assert!(odd_spelling(Path::new("/d/agents/claude-acp/home.")));
}
