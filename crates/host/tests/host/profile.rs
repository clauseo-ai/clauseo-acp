//! The launch profile's host half (R21, R22): what a session gets from the profile hook and
//! the host's own keys, with each session's folders filled in (`profile_rig.rs`), the run
//! folders' life, and the header's tools.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{
    ClauseoTool, StreamId, ToolState, ToolsUnavailableReason, ViewEntry, ViewPatchBody,
};
use serde_json::json;

use crate::profile_rig::{
    CLAUDE, CODEX, Hook, Seen, Wired, agent_lines, agents, browser_server, codex_lines, escaped,
    filled_browser, first_frame_tools, notes, path_text, settings, template, tool, turn,
};
use crate::support::{Dir, FAKE, Scenario};
use crate::write;

/// R22 `work_folder_substituted`: both agents' instructions hold the session's work folder and
/// the matter notes folder, and no token.
#[test]
fn work_folder_substituted() {
    let dir = Dir::new("profile_work_folder", &[]);
    let folder = dir.folder("m", &[]);
    let text = "Work in `{work_folder}`; the brief is {matter_notes}/matter-brief.md.";
    let hook = Hook {
        meta: json!({"systemPrompt": {"append": text}}),
        codex: json!({"developer_instructions": text}),
        ..Hook::default()
    };
    let launcher = Wired::new(&dir);
    // The matter id comes from the store, so the templates are written once it exists.
    let claude = template(&dir, "wf-claude", "");
    let codex = template(&dir, "wf-codex", "");
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({})), (CODEX, &codex, json!({}))]),
        json!({}),
    );
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config.with_profile_hook(Box::new(hook))
    });
    let matter = host.create_matter("m", &folder);
    let notes = path_text(&notes(&dir, matter));
    let expected = format!(
        "Work in `@WORK@`; the brief is {}/matter-brief.md.",
        escaped(&notes)
    );
    template(
        &dir,
        "wf-claude",
        &agent_lines(
            "wf-claude",
            "session/new",
            &json!({"_meta": {"systemPrompt": {"append": expected}}}),
            &[],
        ),
    );
    template(&dir, "wf-codex", &codex_lines("wf-codex", &json!({})));
    turn(&host, host.create_thread(matter, CLAUDE));
    turn(&host, host.create_thread(matter, CODEX));
    host.shutdown();
    let codex = launcher.seen().into_iter().find_map(|seen| {
        let config = seen.codex_config()?;
        Some((seen.work(), config))
    });
    let (work, config) = codex.expect("a Codex launch");
    assert_eq!(
        config["developer_instructions"],
        json!(format!(
            "Work in `{work}`; the brief is {notes}/matter-brief.md."
        ))
    );
    let all = config.to_string();
    assert!(!all.contains("{work_folder}") && !all.contains("{matter_notes}"));
}

/// R22 `writable_folders`: Claude's `additionalDirectories` holds the hook's entries, then the
/// work folder and the matter notes folder; Codex's `writable_roots` holds both beside R15's
/// keys; an agent with no kind gets neither. No other matter's notes folder appears.
#[test]
fn writable_folders() {
    let dir = Dir::new("profile_writable", &[]);
    let launcher = Wired::new(&dir);
    let claude = template(&dir, "wr-claude", "");
    let codex = template(&dir, "wr-codex", "");
    let fake = template(
        &dir,
        "wr-fake",
        &agent_lines(
            "wr-fake",
            "session/new",
            &json!({"_meta": {"claudeCode": {"options": {"strictMcpConfig": true}}}}),
            &[
                "/_meta/claudeCode/options/additionalDirectories",
                "/_meta/claudeCode/options/allowedTools",
                "/_meta/claudeCode/options/disallowedTools",
                "/_meta/claudeCode/options/settings",
                "/_meta/systemPrompt",
            ],
        ),
    );
    settings(
        &dir,
        agents(&[
            (CLAUDE, &claude, json!({})),
            (CODEX, &codex, json!({})),
            (FAKE, &fake, json!({})),
        ]),
        json!({}),
    );
    let hook = Hook {
        meta: json!({"claudeCode": {"options": {"additionalDirectories": ["/firm/precedents"]}}}),
        ..Hook::default()
    };
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config.with_profile_hook(Box::new(hook))
    });
    let other = host.create_matter("other", &dir.folder("other", &[]));
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let notes = path_text(&notes(&dir, matter));
    let other_notes = path_text(&self::notes(&dir, other));
    template(
        &dir,
        "wr-claude",
        &agent_lines(
            "wr-claude",
            "session/new",
            &json!({"_meta": {"claudeCode": {"options": {
                "additionalDirectories": ["/firm/precedents", "@WORK@", notes]}}}}),
            &[],
        ),
    );
    template(&dir, "wr-codex", &codex_lines("wr-codex", &json!({})));
    turn(&host, host.create_thread(matter, CLAUDE));
    turn(&host, host.create_thread(matter, CODEX));
    turn(&host, host.create_thread(matter, FAKE));
    host.shutdown();
    let seen = launcher.seen();
    let (work, config) = seen
        .iter()
        .find_map(|seen| Some((seen.work(), seen.codex_config()?)))
        .expect("a Codex launch");
    assert_eq!(
        config["sandbox_workspace_write"],
        json!({"writable_roots": [work, notes]})
    );
    for (key, value) in [
        ("apps", false),
        ("plugins", false),
        ("skill_mcp_dependency_install", false),
    ] {
        assert_eq!(config["features"][key], json!(value), "features.{key}");
    }
    assert_eq!(config["cloud"]["skills"]["enabled"], json!(false));
    assert!(!config.to_string().contains(&escaped(&other_notes)));
    let codex_launches = seen.iter().filter(|seen| seen.codex_config().is_some());
    assert_eq!(
        codex_launches.count(),
        1,
        "only the Codex agent gets CODEX_CONFIG"
    );
}

/// R22 `skill_shell_off`, and R21's preflight: every Claude session's settings hold
/// `disableSkillShellExecution: true`, with `skipWebFetchPreflight: true` beside it only on a
/// cloud login.
#[test]
fn skill_shell_off() {
    for (test, env, off) in [
        ("profile_shell_first_party", json!({}), true),
        (
            "profile_shell_bedrock",
            json!({"CLAUDE_CODE_USE_BEDROCK": "1"}),
            false,
        ),
        (
            "profile_shell_vertex",
            json!({"CLAUDE_CODE_USE_VERTEX": "true"}),
            false,
        ),
    ] {
        let dir = Dir::new(test, &[]);
        let settings_expected = if off {
            json!({"disableSkillShellExecution": true})
        } else {
            json!({"disableSkillShellExecution": true, "skipWebFetchPreflight": true})
        };
        let offs: &[&str] = if off {
            &["/_meta/claudeCode/options/settings/skipWebFetchPreflight"]
        } else {
            &[]
        };
        let claude = template(
            &dir,
            "shell",
            &agent_lines(
                "shell",
                "session/new",
                &json!({"_meta": {"claudeCode": {"options": {"settings": settings_expected}}}}),
                offs,
            ),
        );
        settings(&dir, agents(&[(CLAUDE, &claude, env)]), json!({}));
        let host = dir.start_launching(0, Wired::new(&dir), |config| config);
        let matter = host.create_matter("m", &dir.folder("m", &[]));
        turn(&host, host.create_thread(matter, CLAUDE));
        host.shutdown();
    }
}

/// R22 `matter_notes_folder`: session start makes the matter's notes folder; two sessions of
/// one matter, Claude and Codex, get the same one; another matter's session gets its own.
#[test]
fn matter_notes_folder() {
    let dir = Dir::new("profile_notes_folder", &[]);
    let launcher = Wired::new(&dir);
    let claude = template(&dir, "nf-claude", "");
    let codex = template(&dir, "nf-codex", "");
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({})), (CODEX, &codex, json!({}))]),
        json!({}),
    );
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    let first = host.create_matter("first", &dir.folder("first", &[]));
    let second = host.create_matter("second", &dir.folder("second", &[]));
    assert!(!notes(&dir, first).exists());
    template(&dir, "nf-codex", &codex_lines("nf-codex", &json!({})));
    for matter in [first, second] {
        let notes = path_text(&notes(&dir, matter));
        template(
            &dir,
            "nf-claude",
            &agent_lines(
                "nf-claude",
                "session/new",
                &json!({"_meta": {"claudeCode": {"options": {
                    "additionalDirectories": ["@WORK@", notes]}}}}),
                &[],
            ),
        );
        turn(&host, host.create_thread(matter, CLAUDE));
        assert!(self::notes(&dir, matter).is_dir());
    }
    turn(&host, host.create_thread(first, CODEX));
    host.shutdown();
    let config = launcher
        .seen()
        .iter()
        .find_map(Seen::codex_config)
        .expect("a Codex launch");
    assert_eq!(
        config["sandbox_workspace_write"]["writable_roots"][1],
        json!(path_text(&notes(&dir, first)))
    );
    assert_ne!(notes(&dir, first), notes(&dir, second));
}

/// R22 `matter_notes_survives_session_end`: the run folder goes when the session ends; the
/// matter's notes folder and what it holds stay for the next session.
#[test]
fn matter_notes_survives_session_end() {
    let dir = Dir::new("profile_notes_survive", &[]);
    settings(
        &dir,
        agents(&[(CLAUDE, &Scenario::Local("mcp-defaults"), json!({}))]),
        json!({}),
    );
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    let brief = notes(&dir, matter).join("matter-brief.md");
    write::write(&brief, b"# Matter brief\n\nParties: A v B.\n");
    host.shutdown();
    let first_run = launcher.seen()[0].run.clone().expect("a run folder");

    let host = dir.start_launching(1, launcher, |config| config);
    turn(&host, host.create_thread(matter, CLAUDE));
    host.shutdown();
    assert!(
        !first_run.exists(),
        "the first session's run folder is gone"
    );
    assert_eq!(
        std::fs::read_to_string(&brief).unwrap(),
        "# Matter brief\n\nParties: A v B.\n"
    );
}

/// The start-up sweep: `run/` folders a crash left are gone once the first launch is prepared,
/// and the live session's folder stays.
#[test]
fn leftover_run_folders_are_swept_at_start_up() {
    let dir = Dir::new("profile_sweep", &[(FAKE, Scenario::Local("mcp-defaults"))]);
    let stale = dir.data().join("run").join("0190-stale-7");
    write::create_dir_all(&stale.join("work"));
    write::write(&stale.join("work").join("notes.md"), b"left over");
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, FAKE));
    assert!(!stale.exists(), "the crash's run folder is swept");
    let left: Vec<_> = std::fs::read_dir(dir.data().join("run"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    let live = launcher.seen().last().and_then(|seen| seen.run.clone());
    assert_eq!(left, live.into_iter().collect::<Vec<_>>());
    host.shutdown();
}

/// The header's tools: a new subscriber's first frame carries the current state, and a
/// relaunch replaces it.
#[test]
fn the_header_shows_the_tools_and_a_relaunch_replaces_them() {
    let dir = Dir::new("profile_header", &[]);
    let new = template(
        &dir,
        "hd-new",
        &agent_lines("hd-new", "session/new", &json!({"mcpServers": []}), &[]),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &new, json!({}))]),
        json!({"tools": {"browser": true}}),
    );
    let hook = Hook {
        servers: vec![browser_server()],
        ..Hook::default()
    };
    let launcher = Wired::new(&dir);
    let hook_now = hook.clone();
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config.with_profile_hook(Box::new(hook_now))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    assert_eq!(
        first_frame_tools(&host, thread),
        [
            tool(ClauseoTool::WebSearch, None),
            tool(ClauseoTool::Browser, Some(ToolsUnavailableReason::NoProxy)),
        ]
    );
    host.shutdown();

    let load = template(
        &dir,
        "hd-load",
        &agent_lines(
            "hd-load",
            "session/load",
            &json!({"sessionId": "sess-1", "mcpServers": [filled_browser()]}),
            &[],
        ),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &load, json!({}))]),
        json!({"tools": {"browser": true}}),
    );
    let host = dir.start_launching(1, launcher, |config| {
        config
            .with_profile_hook(Box::new(hook))
            .with_egress_proxy(Arc::new(clauseo_host::TestProxy(8123)))
            .with_browser_executable(Some(PathBuf::from("/chrome")))
    });
    turn(&host, thread);
    assert_eq!(
        first_frame_tools(&host, thread),
        [
            tool(ClauseoTool::WebSearch, None),
            tool(ClauseoTool::Browser, None)
        ]
    );
    host.shutdown();
}

/// The tools of each header upsert `view` received, in order.
fn upserted_tools(view: &crate::observe::View) -> Vec<Vec<ToolState>> {
    view.patches()
        .filter_map(|patch| match &patch.body {
            ViewPatchBody::Upsert { entry } => match entry.as_ref() {
                ViewEntry::Header(header) => Some(header.tools.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Before its session starts a thread's header shows no tools; once `session/new` answers, a
/// subscriber already open gets them in a header upsert, in the same host run.
#[test]
fn an_open_subscriber_gets_the_tools_once_the_session_starts() {
    let dir = Dir::new("profile_header_live", &[]);
    let new = template(
        &dir,
        "hl-new",
        &agent_lines("hl-new", "session/new", &json!({"mcpServers": []}), &[]),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &new, json!({}))]),
        json!({"tools": {"browser": true}}),
    );
    let hook = Hook {
        servers: vec![browser_server()],
        ..Hook::default()
    };
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, launcher, |config| {
        config.with_profile_hook(Box::new(hook))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    assert_eq!(first_frame_tools(&host, thread), [], "no session, no tools");
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until_items(1);
    turn(&host, thread);
    let shown = vec![
        tool(ClauseoTool::WebSearch, None),
        tool(ClauseoTool::Browser, Some(ToolsUnavailableReason::NoProxy)),
    ];
    view.until("the header's tools", |view| {
        upserted_tools(view).last() == Some(&shown)
    });
    assert!(
        upserted_tools(&view)
            .iter()
            .all(|tools| tools.is_empty() || *tools == shown),
        "{}",
        view.dump()
    );
    host.shutdown();
}

/// A session that never starts (`session/new` answers an error) shows no tools.
#[test]
fn a_failed_session_start_shows_no_tools() {
    let dir = Dir::new("profile_header_failed", &[]);
    settings(
        &dir,
        agents(&[(CLAUDE, &Scenario::Local("new-session-error"), json!({}))]),
        json!({"tools": {"browser": true}}),
    );
    let hook = Hook {
        servers: vec![browser_server()],
        ..Hook::default()
    };
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, launcher, |config| {
        config.with_profile_hook(Box::new(hook))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Go.");
    events.until_turn_ended(turn);
    assert_eq!(
        first_frame_tools(&host, thread),
        [],
        "the failed start shows none"
    );
    host.shutdown();
}
