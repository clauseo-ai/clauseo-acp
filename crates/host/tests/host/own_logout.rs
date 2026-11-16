//! codex-acp's own `logout` never runs (decision record 0048): the host drops it, and
//! `login` with it, from the commands a Codex agent advertises, and refuses a typed `/logout`
//! to a Codex thread with `command.invalid` before anything is queued or sent. Signing out
//! goes through Clauseo's `SignOut` alone.

use std::path::PathBuf;

use clauseo_protocol::{DomainEvent, ErrorCode, StreamId};

use crate::codex::{CODEX, run_turn};
use crate::support::{Dir, Scenario, matter_with_thread, send_prompt};

/// codex-acp's handshake, then one prompt during which it advertises `review`, `logout` and
/// `login`.
const TURN: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"review","description":"Review the matter"},{"name":"logout","description":"Log out of Codex"},{"name":"Login","description":"Log in to Codex"}]}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
"#;

/// Writes the scenario beside a copy of `fake_agent`'s `common/codex-handshake.jsonl`, which
/// its `include` reads.
fn scenario(dir: &Dir) -> Scenario {
    let common = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fake_agent/scenarios/common/codex-handshake.jsonl"
    );
    let handshake = std::fs::read(common).unwrap();
    crate::write::write(
        &dir.home().join("common").join("codex-handshake.jsonl"),
        &handshake,
    );
    let header = r#"{"scenario":"codex-logout","description":"Codex advertises review, logout and login during its one prompt","on_stdin_close":"exit"}"#;
    let text = format!("{header}\n{{\"include\":\"codex-handshake.jsonl\"}}\n{TURN}");
    let path: PathBuf = dir.home().join("codex-logout.jsonl");
    crate::write::write(&path, text.as_bytes());
    Scenario::Written(path)
}

#[test]
fn codex_logout_and_login_are_hidden_and_a_typed_logout_is_refused() {
    let dir = Dir::new("own_logout", &[]);
    dir.set_agents(&[(CODEX, scenario(&dir))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));

    for typed in ["/logout", "  /LOGOUT now"] {
        let error = host.refused(send_prompt(thread, typed), ErrorCode::CommandInvalid);
        assert!(error.message().contains("Plans"), "{error}");
    }
    // Nothing was queued or sent: the one prompt the agent expects is the next one.
    run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    let advertised: Vec<Vec<String>> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::CommandsAvailable(available) => Some(
                available
                    .commands
                    .iter()
                    .map(|command| command.name.to_string())
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    assert_eq!(advertised, [vec!["review".to_owned()]]);
    host.shutdown();
}
