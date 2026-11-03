//! R21's switches and conditions at the session: `tools.web_search` and `tools.browser` (set by
//! the lawyer, locked by policy, or locked off by a policy source that cannot be read), the
//! firm's `managed-mcp.json`, a Bedrock login, an agent with no kind, the skills written before
//! the launch, and stetdoc's folder on `PATH`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_host::{HostConfig, TestProxy};
use clauseo_profile::{BrowserSidecars, ClauseoProfile, ProfilePaths, SkillFile, SkillSource};
use clauseo_protocol::{AgentKind, ClauseoTool, ToolsUnavailableReason};
use clauseo_settings::Locations;
use serde_json::{Value, json};

use crate::profile_rig::{
    CLAUDE, CODEX, Hook, Wired, agent_lines, agents, browser_server, codex_lines, filled_browser,
    first_frame_tools, settings, template, tool, turn,
};
use crate::support::{Dir, FAKE};
use crate::write;

/// The seeded hosts, written out: a change to the seed list changes Claude's allow rules, so it
/// changes this test too.
const SEEDED: [&str; 33] = [
    "sci.gov.in",
    "egazette.gov.in",
    "cci.gov.in",
    "sebi.gov.in",
    "rbi.org.in",
    "ibbi.gov.in",
    "mca.gov.in",
    "indiacode.nic.in",
    "allahabadhighcourt.in",
    "aphc.gov.in",
    "bombayhighcourt.nic.in",
    "calcuttahighcourt.gov.in",
    "highcourt.cg.gov.in",
    "delhihighcourt.nic.in",
    "ghconline.gov.in",
    "gujarathighcourt.nic.in",
    "hphighcourt.nic.in",
    "jkhighcourt.nic.in",
    "jharkhandhighcourt.nic.in",
    "judiciary.karnataka.gov.in",
    "highcourt.kerala.gov.in",
    "mphc.gov.in",
    "hcmadras.tn.gov.in",
    "hcmimphal.nic.in",
    "meghalayahighcourt.nic.in",
    "orissahighcourt.nic.in",
    "patnahighcourt.gov.in",
    "highcourtchd.gov.in",
    "hcraj.nic.in",
    "hcs.gov.in",
    "tshc.gov.in",
    "thc.nic.in",
    "highcourtofuttarakhand.gov.in",
];

/// The `allowedTools` a Claude session gets with no hook and web search on.
fn seeded(web_search: bool) -> Vec<String> {
    web_search
        .then(|| "WebSearch".to_owned())
        .into_iter()
        .chain(SEEDED.map(|host| format!("WebFetch(domain:{host})")))
        .collect()
}

fn with_browser(config: HostConfig, hook: Hook) -> HostConfig {
    config
        .with_profile_hook(Box::new(hook))
        .with_egress_proxy(Arc::new(TestProxy(8123)))
        .with_browser_executable(Some(PathBuf::from("/chrome")))
}

fn browser_hook() -> Hook {
    Hook {
        servers: vec![browser_server()],
        ..Hook::default()
    }
}

/// One Claude thread and one Codex thread, each one turn; returns the Codex `CODEX_CONFIG` and
/// the Claude thread's header tools.
fn both(
    dir: &Dir,
    claude_params: &Value,
    claude_off: &[&str],
    codex_params: &Value,
    extra: Value,
    adjust: impl FnOnce(HostConfig) -> HostConfig,
) -> (Value, Vec<clauseo_protocol::ToolState>) {
    let claude = template(
        dir,
        "claude",
        &agent_lines("claude", "session/new", claude_params, claude_off),
    );
    let codex = template(dir, "codex", &codex_lines("codex", codex_params));
    settings(
        dir,
        agents(&[(CLAUDE, &claude, json!({})), (CODEX, &codex, json!({}))]),
        extra,
    );
    let launcher = Wired::new(dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), adjust);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    turn(&host, host.create_thread(matter, CODEX));
    let tools = first_frame_tools(&host, thread);
    host.shutdown();
    let config = launcher
        .seen()
        .iter()
        .find_map(crate::profile_rig::Seen::codex_config)
        .expect("a Codex launch");
    (config, tools)
}

fn off_claude() -> Value {
    json!({"_meta": {"claudeCode": {"options": {
        "disallowedTools": ["WebSearch", "WebFetch"], "allowedTools": []}}}})
}

#[test]
fn web_search_off_reaches_both_agents() {
    let dir = Dir::new("tools_search_off", &[]);
    let (config, tools) = both(
        &dir,
        &off_claude(),
        &[],
        &json!({}),
        json!({"tools": {"web_search": false}}),
        |config| config,
    );
    assert_eq!(config["web_search"], json!("disabled"));
    assert_eq!(
        tools,
        [tool(
            ClauseoTool::WebSearch,
            Some(ToolsUnavailableReason::Off)
        )]
    );
}

/// Policy's `false`, and a policy source that cannot be read, both lock web search off for
/// both agents.
#[test]
fn locked_off_web_search_reaches_both_agents() {
    for (test, policy) in [
        ("tools_search_policy", r#"{"tools": {"web_search": false}}"#),
        ("tools_search_unreadable", "{ not json"),
    ] {
        let dir = Dir::new(test, &[]);
        dir.set_policy(policy);
        let (config, tools) = both(
            &dir,
            &off_claude(),
            &[],
            &json!({}),
            json!({"tools": {"web_search": true}}),
            |config| config,
        );
        assert_eq!(config["web_search"], json!("disabled"), "{test}");
        assert_eq!(
            tools,
            [tool(
                ClauseoTool::WebSearch,
                Some(ToolsUnavailableReason::LockedOff)
            )],
            "{test}"
        );
    }
}

#[test]
fn web_search_on_allows_search_and_the_seeded_sites() {
    let dir = Dir::new("tools_search_on", &[]);
    let (config, tools) = both(
        &dir,
        &json!({"_meta": {"claudeCode": {"options": {"allowedTools": seeded(true)}}}}),
        &["/_meta/claudeCode/options/disallowedTools"],
        &json!({}),
        json!({}),
        |config| config,
    );
    assert_eq!(config["web_search"], json!("live"));
    assert_eq!(tools, [tool(ClauseoTool::WebSearch, None)]);
}

/// A Bedrock login has no `WebSearch`: the header says so, `WebFetch` stays allowed, and the
/// preflight is skipped.
#[test]
fn a_bedrock_login_shows_no_search() {
    let dir = Dir::new("tools_bedrock", &[]);
    let claude = template(
        &dir,
        "bedrock",
        &agent_lines(
            "bedrock",
            "session/new",
            &json!({"_meta": {"claudeCode": {"options": {
                "allowedTools": seeded(false),
                "settings": {"disableSkillShellExecution": true, "skipWebFetchPreflight": true}}}}}),
            &["/_meta/claudeCode/options/disallowedTools"],
        ),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({"CLAUDE_CODE_USE_BEDROCK": "1"}))]),
        json!({}),
    );
    let host = dir.start_launching(0, Wired::new(&dir), |config| config);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    assert_eq!(
        first_frame_tools(&host, thread),
        [tool(
            ClauseoTool::WebSearch,
            Some(ToolsUnavailableReason::NoSearchOnThisLogin)
        )]
    );
    host.shutdown();
}

/// `tools.browser: false` removes the browser for both agents: `Off`, or `LockedOff` from
/// policy. Web search is untouched.
#[test]
fn the_browser_switch_removes_the_browser() {
    for (test, user, policy, reason) in [
        (
            "tools_browser_off",
            false,
            None,
            ToolsUnavailableReason::Off,
        ),
        (
            "tools_browser_policy",
            true,
            Some(r#"{"tools": {"browser": false}}"#),
            ToolsUnavailableReason::LockedOff,
        ),
    ] {
        let dir = Dir::new(test, &[]);
        if let Some(policy) = policy {
            dir.set_policy(policy);
        }
        let (config, tools) = both(
            &dir,
            &json!({"mcpServers": []}),
            &[],
            &json!({"mcpServers": []}),
            json!({"tools": {"browser": user}}),
            |config| with_browser(config, browser_hook()),
        );
        assert_eq!(config["web_search"], json!("live"));
        assert_eq!(
            tools,
            [
                tool(ClauseoTool::WebSearch, None),
                tool(ClauseoTool::Browser, Some(reason)),
            ],
            "{test}"
        );
    }
}

/// R21's done-when "Managed MCP": with a `managed-mcp.json`, no session gets any server,
/// Clauseo's included, and the header says the firm manages it. Web search follows its switch.
#[test]
fn under_managed_mcp_no_agent_gets_any_server() {
    let dir = Dir::new("tools_managed", &[]);
    let bin = dir.folder("bin", &[]);
    dir.set_managed_mcp();
    let (config, tools) = both(
        &dir,
        &json!({"mcpServers": [], "_meta": {"claudeCode": {"options": {
            "strictMcpConfig": false, "settingSources": ["user"]}}}}),
        &[],
        &json!({"mcpServers": []}),
        json!({"tools": {"browser": true}, "mcp": {"servers": {
            "alpha": {"command": bin.join("alpha").display().to_string(), "enabled": true}}}}),
        |config| with_browser(config, browser_hook()),
    );
    assert_eq!(config["web_search"], json!("live"));
    assert_eq!(
        tools,
        [
            tool(ClauseoTool::WebSearch, None),
            tool(
                ClauseoTool::Browser,
                Some(ToolsUnavailableReason::ManagedByFirm)
            ),
        ]
    );
}

/// With no proxy the browser is left out for both agents (`NoProxy`); web search is unaffected.
#[test]
fn no_proxy_no_browser() {
    let dir = Dir::new("tools_no_proxy", &[]);
    let (config, tools) = both(
        &dir,
        &json!({"mcpServers": []}),
        &[],
        &json!({"mcpServers": []}),
        json!({"tools": {"browser": true}}),
        |config| {
            config
                .with_profile_hook(Box::new(browser_hook()))
                .with_browser_executable(Some(PathBuf::from("/chrome")))
        },
    );
    assert_eq!(config["web_search"], json!("live"));
    assert_eq!(
        tools,
        [
            tool(ClauseoTool::WebSearch, None),
            tool(ClauseoTool::Browser, Some(ToolsUnavailableReason::NoProxy)),
        ]
    );
}

/// A profile whose one skill is a small stetdoc stand-in, its folders under `root`.
fn profile(root: &Path) -> ClauseoProfile {
    let paths = ProfilePaths::new(
        &Locations::under_root(root),
        root.join("app").join("clauseo"),
        root.join("stetdoc").join("stetdoc"),
        BrowserSidecars {
            clauseo_mcp: root.join("app").join("clauseo-mcp"),
            agent_browser: root.join("app").join("agent-browser"),
        },
    )
    .unwrap();
    let skill = SkillSource::verbatim(
        "stetdoc",
        "0.9.1",
        vec![SkillFile {
            path: "SKILL.md".to_owned(),
            bytes: b"---\nname: stetdoc\ndescription: \"Edits Word files as tracked changes.\"\n---\n# stetdoc\n"
                .to_vec(),
        }],
    );
    let (profile, refused) = ClauseoProfile::new(paths, None).with_skills(&[skill]);
    assert!(refused.is_empty(), "{refused:?}");
    profile
}

/// An agent with no kind gets Clauseo's servers and none of the kind-specific keys: no
/// `CODEX_CONFIG`, no R21 Claude keys, no skill written.
#[test]
fn an_agent_with_no_kind_gets_the_servers_and_nothing_else() {
    let dir = Dir::new("tools_no_kind", &[]);
    let fake = template(
        &dir,
        "fake",
        &agent_lines(
            "fake",
            "session/new",
            &json!({"mcpServers": [filled_browser()]}),
            &[
                "/_meta/claudeCode/options/allowedTools",
                "/_meta/claudeCode/options/disallowedTools",
                "/_meta/claudeCode/options/additionalDirectories",
                "/_meta/claudeCode/options/settings",
            ],
        ),
    );
    settings(
        &dir,
        agents(&[(FAKE, &fake, json!({}))]),
        json!({"tools": {"browser": true}}),
    );
    let skills = profile(dir.data()).launch_writes(Some(AgentKind::Claude));
    let hook = Hook {
        skills,
        ..browser_hook()
    };
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        with_browser(config, hook)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, FAKE));
    host.shutdown();
    assert!(
        launcher
            .seen()
            .iter()
            .all(|seen| seen.codex_config().is_none())
    );
    assert!(!dir.data().join("profile").exists(), "no skill is written");
}

/// The skills are written before a Claude or Codex agent starts; a write that fails is a
/// warning and the agent still starts.
#[test]
fn skills_are_written_before_the_launch_and_a_failure_is_no_failed_launch() {
    let dir = Dir::new("tools_skills", &[]);
    let claude = template(
        &dir,
        "skills",
        &agent_lines("skills", "session/new", &json!({}), &[]),
    );
    let codex = template(
        &dir,
        "skills-codex",
        &codex_lines("skills-codex", &json!({})),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({})), (CODEX, &codex, json!({}))]),
        json!({}),
    );
    let skill_md = dir
        .data()
        .join("profile")
        .join("claude-plugin")
        .join("skills")
        .join("stetdoc")
        .join("SKILL.md");
    let skills = profile(dir.data()).launch_writes(Some(AgentKind::Claude));
    let host = dir.start_launching(0, Wired::new(&dir), |config| {
        config.with_profile_hook(Box::new(Hook {
            skills: skills.clone(),
            ..Hook::default()
        }))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CODEX));
    assert!(skill_md.is_file(), "a Codex launch writes the skills too");
    // The skills folder made a file: every write fails, and the launch still goes.
    write::remove_dir_all(
        &dir.data()
            .join("profile")
            .join("claude-plugin")
            .join("skills"),
    )
    .unwrap();
    write::write(
        &dir.data()
            .join("profile")
            .join("claude-plugin")
            .join("skills"),
        b"not a folder",
    );
    turn(&host, host.create_thread(matter, CLAUDE));
    host.shutdown();
}

/// Both agents launch with stetdoc's folder first on `PATH`.
#[test]
fn stetdoc_goes_first_on_both_agents_path() {
    let dir = Dir::new("tools_path", &[]);
    let claude = template(
        &dir,
        "path",
        &agent_lines("path", "session/new", &json!({}), &[]),
    );
    let codex = template(&dir, "path-codex", &codex_lines("path-codex", &json!({})));
    settings(
        &dir,
        agents(&[
            (CLAUDE, &claude, json!({"PATH": "/opt/firm/bin"})),
            (CODEX, &codex, json!({})),
        ]),
        json!({}),
    );
    let folder = dir.data().join("stetdoc");
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config.with_agent_path_prepend(folder.clone())
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    turn(&host, host.create_thread(matter, CODEX));
    host.shutdown();
    let seen = launcher.seen();
    assert_eq!(seen.len(), 2);
    for (at, launch) in seen.iter().enumerate() {
        let (_, path) = launch
            .env
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
            .expect("a PATH");
        let separator = if cfg!(windows) { ';' } else { ':' };
        let parts: Vec<PathBuf> = path
            .to_str()
            .unwrap()
            .split(separator)
            .map(PathBuf::from)
            .collect();
        assert_eq!(parts.first(), Some(&folder), "launch {at}");
        if at == 0 {
            assert_eq!(parts.get(1), Some(&PathBuf::from("/opt/firm/bin")));
            assert_eq!(parts.len(), 2);
        }
    }
}

/// A Codex launch's keys carry the skills index only from that launch's own write (0031):
/// after a failed write its instructions have no index, and it never gets an earlier launch's
/// token.
#[test]
fn a_failed_skills_write_gives_codex_no_index_and_no_earlier_token() {
    let dir = Dir::new("tools_skills_token", &[]);
    let codex = template(&dir, "token-codex", &codex_lines("token-codex", &json!({})));
    settings(&dir, agents(&[(CODEX, &codex, json!({}))]), json!({}));
    let profile = Arc::new(profile(dir.data()));
    let index = profile.skills_index();
    assert_ne!(index, "");
    let hook = Hook {
        skills: profile.launch_writes(Some(AgentKind::Codex)),
        profile: Some(Arc::clone(&profile)),
        ..Hook::default()
    };
    let tokens = Arc::clone(&hook.tokens);
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config.with_profile_hook(Box::new(hook))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CODEX));
    // The skills folder made a file: the next launch's write fails.
    let skills = dir
        .data()
        .join("profile")
        .join("claude-plugin")
        .join("skills");
    write::remove_dir_all(&skills).unwrap();
    write::write(&skills, b"not a folder");
    turn(&host, host.create_thread(matter, CODEX));
    host.shutdown();
    let instructions: Vec<String> = launcher
        .seen()
        .iter()
        .map(|seen| {
            let config = seen.codex_config().expect("a CODEX_CONFIG");
            config["developer_instructions"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(instructions.len(), 2);
    assert!(instructions[0].contains(&index), "{}", instructions[0]);
    assert!(!instructions[1].contains(&index), "{}", instructions[1]);
    let tokens = tokens.lock().unwrap();
    assert!(matches!(tokens.as_slice(), [Some(_), None]), "{tokens:?}");
}
