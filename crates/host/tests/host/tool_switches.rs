//! A tool switch the lawyer changes while a session runs (`SetToolSettings`): the running session
//! keeps the tools its header shows, and the next session started gets the change. The rest of
//! R21's switches are in `tools.rs`.

use std::sync::Arc;

use clauseo_protocol::{ClauseoTool, HostCommand, ToolsPatch, ToolsUnavailableReason};
use serde_json::json;

use crate::profile_rig::{
    CLAUDE, Wired, agent_lines, agents, first_frame_tools, settings, template, tool, turn,
};
use crate::support::Dir;

/// A switch changed while a session runs reaches the next session started, not the one running:
/// the running session's header keeps the tools it started with, and the next session's shows
/// the change.
#[test]
fn a_running_session_keeps_its_tools_until_the_next_session_starts() {
    let dir = Dir::new("tools_running_session", &[]);
    // Only the `_meta` object is checked, so the two sessions' differing allowed lists match.
    let claude = template(
        &dir,
        "claude",
        &agent_lines(
            "claude",
            "session/new",
            &json!({"_meta": {"claudeCode": {"options": {}}}}),
            &[],
        ),
    );
    settings(&dir, agents(&[(CLAUDE, &claude, json!({}))]), json!({}));
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let running = host.create_thread(matter, CLAUDE);
    turn(&host, running);
    let on = [tool(ClauseoTool::WebSearch, None)];
    assert_eq!(first_frame_tools(&host, running), on);

    // The lawyer turns web search off while the session runs.
    host.ok(HostCommand::SetToolSettings {
        patch: ToolsPatch {
            web_search: Some(false),
            browser: None,
        },
    });
    // The running session's header still shows what it started with, to a new subscriber too.
    assert_eq!(first_frame_tools(&host, running), on);

    // The next session started gets the change.
    let next = host.create_thread(matter, CLAUDE);
    turn(&host, next);
    assert_eq!(
        first_frame_tools(&host, next),
        [tool(
            ClauseoTool::WebSearch,
            Some(ToolsUnavailableReason::Off)
        )]
    );
    assert_eq!(first_frame_tools(&host, running), on);
    host.shutdown();
}
