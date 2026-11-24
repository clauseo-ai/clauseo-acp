//! What a launch gets in each matter once a skill or tool is scoped or switched (decision
//! record 0040, Scope and per-matter switches): a pack on in X only is in X's Claude plugin and
//! Codex index and not Y's; a server on in X only is in X's `session/new` `mcpServers` and not
//! Y's; and switching a server or pack off in X stops X's idle agent to relaunch it
//! (`Relaunch::ToolsChanged`, detached `IdleStopped`) while Y's is left running.

use std::path::Path;
use std::sync::Arc;

use clauseo_protocol::{AddonKind, AddonScope, MatterId, StreamId, ThreadId, ThreadStatus};
use serde_json::{Value, json};

use super::scope::{scope, set_on, two_matters};
use crate::idle_stop::{LIMIT, idle_stopped};
use crate::mcp_tools::{finds_npx, npx, store, wired};
use crate::packs::{PackHook, profile, put_pack, skills_on};
use crate::profile_rig::{CLAUDE, CODEX, Wired, agent_lines, agents, codex_lines, settings};
use crate::profile_rig::{template, turn};
use crate::support::{Dir, FAKE, Running, Scenario, lawyer_env};

/// A second generic agent, whose scenario expects Y's launch.
const OTHER: &str = "fake-agent-y";

/// The `PATH` the host is given on unix (`mcp_tools::wired`).
// unix-paths-ok: a PATH value handed to the host as text, never a path API, as in mcp_tools.rs
const INHERITED_PATH: &str = "/usr/bin:/bin";

/// A generic or Claude agent's scenario that takes any `session/new` and answers one prompt.
fn accepting(dir: &Dir, name: &str) -> Scenario {
    template(
        dir,
        name,
        &agent_lines(name, "session/new", &json!({}), &[]),
    )
}

/// A generic agent's scenario whose `session/new` carries exactly `servers`.
fn with_servers(dir: &Dir, name: &str, servers: &Value) -> Scenario {
    let params = json!({"cwd": "{{any}}", "mcpServers": servers});
    template(dir, name, &agent_lines(name, "session/new", &params, &[]))
}

/// The `env` a local server whose program is `command` gets with no variables of its own.
fn server_env(command: &Path) -> Value {
    if cfg!(unix) {
        let folder = command.parent().unwrap().display().to_string();
        json!([{"name": "PATH", "value": format!("{folder}:{INHERITED_PATH}")}])
    } else {
        lawyer_env(Vec::new())
    }
}

/// The skill folders a launch's pack plugin held.
fn pack_skills(packs: &[String]) -> Vec<String> {
    let mut names: Vec<String> = packs
        .iter()
        .filter_map(|file| file.strip_prefix("skills/"))
        .filter_map(|rest| rest.split('/').next())
        .map(str::to_owned)
        .collect();
    names.dedup();
    names
}

/// A pack scoped to X: X's Claude launch gets it as its pack plugin, Y's gets none.
#[test]
fn a_pack_on_in_one_matter_goes_to_its_claude_launch_only() {
    let dir = Dir::new("addons_scope_packs_claude", &[]);
    let hash = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let claude = accepting(&dir, "scope-packs-claude");
    let entries = agents(&[(CLAUDE, &claude, json!({}))]);
    settings(&dir, entries, skills_on(&[("nda-kit", &hash)]));
    let launcher = Wired::new(&dir);
    let host = crate::packs::start(&dir, 0, &launcher);
    let (x, y) = two_matters(&dir, &host);
    host.ok(scope(AddonKind::Skill, "nda-kit", AddonScope::Matter(x)));
    turn(&host, host.create_thread(x, CLAUDE));
    turn(&host, host.create_thread(y, CLAUDE));
    host.shutdown();
    let seen = launcher.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(pack_skills(&seen[0].packs), ["nda-kit--nda-review"]);
    assert!(seen[1].packs.is_empty(), "{:?}", seen[1].packs);
}

/// A pack switched off in Y (scope `all`): X's Codex index names it, Y's does not.
#[test]
fn a_pack_switched_off_in_a_matter_leaves_its_codex_index() {
    let dir = Dir::new("addons_scope_packs_codex", &[]);
    let hash = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let codex = template(
        &dir,
        "scope-packs-codex",
        &codex_lines("scope-packs-codex", &json!({})),
    );
    let entries = agents(&[(CODEX, &codex, json!({}))]);
    settings(&dir, entries, skills_on(&[("nda-kit", &hash)]));
    let launcher = Wired::new(&dir);
    let host = crate::packs::start(&dir, 0, &launcher);
    let (x, y) = two_matters(&dir, &host);
    host.ok(set_on(AddonKind::Skill, "nda-kit", y, false));
    turn(&host, host.create_thread(x, CODEX));
    turn(&host, host.create_thread(y, CODEX));
    host.shutdown();
    let seen = launcher.seen();
    assert_eq!(seen.len(), 2);
    let index = |at: usize| {
        let config = seen[at].codex_config().expect("a CODEX_CONFIG");
        config["developer_instructions"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    assert!(index(0).contains("nda-kit--nda-review"), "{}", index(0));
    assert!(!index(1).contains("nda-kit--"), "{}", index(1));
    assert_eq!(pack_skills(&seen[0].packs), ["nda-kit--nda-review"]);
    assert!(seen[1].packs.is_empty(), "{:?}", seen[1].packs);
}

/// A server scoped to X: X's `session/new` carries it, Y's carries none (each scenario checks
/// `mcpServers` exactly).
#[test]
fn a_server_on_in_one_matter_goes_to_its_session_only() {
    let dir = Dir::new("addons_scope_servers", &[]);
    let npx = npx(&dir);
    let server = json!([{"name": "github", "command": npx.display().to_string(),
        "args": ["-y", "server-github"], "env": server_env(&npx)}]);
    let in_x = with_servers(&dir, "scope-server-x", &server);
    let in_y = with_servers(&dir, "scope-server-y", &json!([]));
    dir.set_settings(
        &[(FAKE, in_x), (OTHER, in_y)],
        json!({"mcp": {"servers": {"github": {"command": "npx",
            "args": ["-y", "server-github"], "enabled": true}}}}),
    );
    let store = store(&[]);
    let host = dir.start_with(0, |config| wired(config, &store, finds_npx(&npx)));
    let (x, y) = two_matters(&dir, &host);
    host.ok(scope(AddonKind::Tool, "github", AddonScope::Matter(x)));
    turn(&host, host.create_thread(x, FAKE));
    turn(&host, host.create_thread(y, OTHER));
    host.shutdown();
}

/// After a change, X's idle agent is stopped at the Tools grace (`IdleStopped`) and Y's is
/// still idle and running.
fn only_x_stops(host: &Running, (x, in_x): (MatterId, ThreadId), (y, in_y): (MatterId, ThreadId)) {
    let mut events = host.events(StreamId::Thread(in_x));
    host.wait_armed(LIMIT);
    host.clock.advance(LIMIT);
    events.until("X's agent stopped", |events| {
        events.position(idle_stopped).is_some()
    });
    host.settle();
    assert_eq!(host.status(x, in_x), ThreadStatus::IdleStopped);
    let mut other = host.events(StreamId::Thread(in_y));
    other.until_caught_up();
    assert_eq!(other.count(idle_stopped), 0, "{}", other.dump());
    assert_eq!(host.status(y, in_y), ThreadStatus::Idle);
}

/// A server switched off in X while an agent idles in each matter: X's is marked and stopped to
/// relaunch, Y's is not.
#[test]
fn switching_a_server_off_in_a_matter_stops_only_its_idle_agent() {
    let dir = Dir::new("addons_scope_idle_server", &[]);
    let npx = npx(&dir);
    let scenario = accepting(&dir, "scope-idle-server");
    dir.set_settings(
        &[(FAKE, scenario)],
        json!({"mcp": {"servers": {"github": {"command": "npx", "enabled": true}}}}),
    );
    let store = store(&[]);
    let host = dir.start_with(0, |config| {
        wired(config, &store, finds_npx(&npx)).with_cap_min_idle(LIMIT)
    });
    let (x, y) = two_matters(&dir, &host);
    let (in_x, in_y) = (host.create_thread(x, FAKE), host.create_thread(y, FAKE));
    turn(&host, in_x);
    turn(&host, in_y);
    host.ok(set_on(AddonKind::Tool, "github", x, false));
    only_x_stops(&host, (x, in_x), (y, in_y));
    host.shutdown();
}

/// A pack switched off in X while a Claude agent idles in each matter: X's is marked and
/// stopped to relaunch, Y's is not.
#[test]
fn switching_a_pack_off_in_a_matter_stops_only_its_idle_agent() {
    let dir = Dir::new("addons_scope_idle_pack", &[]);
    let hash = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let claude = accepting(&dir, "scope-idle-pack");
    let entries = agents(&[(CLAUDE, &claude, json!({}))]);
    settings(&dir, entries, skills_on(&[("nda-kit", &hash)]));
    let launcher = Wired::new(&dir);
    let hook = PackHook(Arc::new(profile(dir.data())));
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config
            .with_profile_hook(Box::new(hook))
            .with_cap_min_idle(LIMIT)
    });
    let (x, y) = two_matters(&dir, &host);
    let (in_x, in_y) = (host.create_thread(x, CLAUDE), host.create_thread(y, CLAUDE));
    turn(&host, in_x);
    turn(&host, in_y);
    let seen = launcher.seen();
    assert!(
        seen.iter().all(|launch| !launch.packs.is_empty()),
        "{seen:?}"
    );
    host.ok(set_on(AddonKind::Skill, "nda-kit", x, false));
    only_x_stops(&host, (x, in_x), (y, in_y));
    host.shutdown();
}
