//! Claude's own plan note through the host (`agent/plan_note.rs`): in a matter in forced ask, a
//! Claude agent's request to write `<CLAUDE_CONFIG_DIR>/plans/<name>.md` is logged with
//! `about: agent_plan`, and its request for the matter's own `plans/<name>.md` without it. The same
//! requests from an agent that is not Claude carry no mark. Which other paths are refused (nested,
//! another extension, `..`, Windows spellings) is pinned in `agent/plan_note/tests.rs`.

use std::ffi::OsString;

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, DomainEvent, FolderSupportKind, HostCommand, PermissionAbout,
    StopReason, StreamId,
};

use crate::support::{Dir, Scenario, matter_with_thread};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// An agent id of no known kind.
const OTHER: &str = "fake-agent";

/// The guard program the tests' hosts name; the fake agent never runs it.
const GUARD: &str = "/opt/clauseo/clauseo-desktop";

/// Runs `claude-plan-note` on an `agent` thread in a matter on a network drive, with
/// `CLAUDE_CONFIG_DIR` set to `<matter folder>/claude-config` in the agents' environment,
/// answers both requests allow-once, and returns each logged request's `about`.
fn abouts(test: &str, agent: &str) -> Vec<Option<PermissionAbout>> {
    let dir = Dir::new(test, &[(agent, Scenario::Local("claude-plan-note"))]);
    let files = [("brief.txt", "x\n")];
    let folder = dir.folder("m", &files);
    // The spelling the agent's `{{cwd}}` has: the folder the session runs in.
    let config = clauseo_host::session_folder(&folder).join("claude-config");
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("CLAUDE_CONFIG_DIR"))
        .collect();
    env.push((OsString::from("CLAUDE_CONFIG_DIR"), config.into_os_string()));
    let host = dir.start_with(0, |config| {
        config
            .with_folder_support(Some(FolderSupportKind::Network))
            .with_pre_write_guard(std::path::PathBuf::from(GUARD))
            .with_inherited_env(env)
    });
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &files, agent);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Plan the lease review.");
    for n in 1..=2 {
        events.until(&format!("permission request {n}"), |e| {
            e.permission_requests().len() == n
        });
        let answer = HostCommand::AnswerPermission {
            thread,
            request: events.permission_requests()[n - 1],
            option_id: AgentChoiceId::new("allow-once").unwrap(),
        };
        assert_eq!(host.ok(answer), CommandOutcome::Done, "{}", events.dump());
    }
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    events.assert_no_mismatch();
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PermissionRequested(asked) => Some(asked.about),
            _ => None,
        })
        .collect()
}

#[test]
fn claudes_plan_note_is_marked_and_the_matters_own_plans_file_is_not() {
    assert_eq!(
        abouts("plan_note_claude", CLAUDE),
        [Some(PermissionAbout::AgentPlan), None]
    );
}

#[test]
fn the_same_requests_from_another_agent_are_not_marked() {
    assert_eq!(abouts("plan_note_other", OTHER), [None, None]);
}
