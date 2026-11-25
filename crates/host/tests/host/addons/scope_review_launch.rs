//! What a launch gets after review fixes (decision record 0040): a server scoped to X and then
//! edited through Settings › Tools (a new keychain value, a new program) is still X's alone; and
//! a matter that switches Clauseo's `browser` and `case-law` off gives its Claude and Codex
//! launches neither entry, opens them no case-law relay and no egress proxy, while another
//! matter's launches keep both.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_host::TestProxy;
use clauseo_protocol::{AddonKind, AddonScope, ClauseoTool, MatterId};
use serde_json::{Value, json};

use super::scope::{row, scope, set_on, two_matters};
use crate::case_law::{Connector, claude_params, filled_case_law, token_of, wire};
use crate::mcp_tools::{HELD, Resolver, local, save, set, store, wired};
use crate::profile_rig::{
    CLAUDE, CODEX, Seen, Wired, agent_lines, agents, codex_lines, codex_servers, filled_browser,
    first_frame_tools, settings, template, turn,
};
use crate::support::{Dir, FAKE, Running, Scenario, lawyer_env};

/// A second generic agent, whose scenario expects Y's launch.
const OTHER: &str = "fake-agent-y";

/// The `PATH` the host is given on unix (`mcp_tools::wired`).
// unix-paths-ok: a PATH value handed to the host as text, never a path API, as in mcp_tools.rs
const INHERITED_PATH: &str = "/usr/bin:/bin";

/// A generic agent's scenario whose `session/new` carries exactly `servers`.
pub(super) fn with_servers(dir: &Dir, name: &str, servers: &Value) -> Scenario {
    let params = json!({"cwd": "{{any}}", "mcpServers": servers});
    template(dir, name, &agent_lines(name, "session/new", &params, &[]))
}

/// The `env` a local server whose program is `command` gets: `own`, then its `PATH` (the
/// program's folder, then the inherited one) on unix, and as `lawyer_env` has it on Windows.
pub(super) fn server_env(command: &Path, own: Vec<Value>) -> Value {
    if cfg!(unix) {
        let folder = command.parent().unwrap().display().to_string();
        let mut env = own;
        env.push(json!({"name": "PATH", "value": format!("{folder}:{INHERITED_PATH}")}));
        Value::Array(env)
    } else {
        lawyer_env(own)
    }
}

/// `<work>/bin/npx` and `<work>/bin/uvx`, empty files nothing launches, and a resolver that
/// finds each there and nothing else.
pub(super) fn programs(dir: &Dir) -> (PathBuf, PathBuf, Resolver) {
    let bin = dir.folder("bin", &[("npx", ""), ("uvx", "")]);
    let (npx, uvx) = (bin.join("npx"), bin.join("uvx"));
    let resolver: Resolver =
        Arc::new(move |typed: &str| matches!(typed, "npx" | "uvx").then(|| bin.join(typed)));
    (npx, uvx, resolver)
}

/// The settings file as JSON.
fn settings_file(dir: &Dir) -> Value {
    serde_json::from_str(&dir.settings_json()).unwrap()
}

/// `github` is still on in X alone after its edit: scope X, switched `[X]`, `matter` in its
/// settings entry.
pub(super) fn still_scoped(dir: &Dir, host: &Running, (x, y): (MatterId, MatterId)) {
    let in_x = row(host, Some(x), AddonKind::Tool, "github");
    assert!(in_x.on, "{in_x:?}");
    assert_eq!(in_x.scope, AddonScope::Matter(x));
    assert_eq!(in_x.switched, [x]);
    assert!(!row(host, Some(y), AddonKind::Tool, "github").on);
    let file = settings_file(dir);
    assert_eq!(
        file["mcp"]["servers"]["github"]["scope"], "matter",
        "{file}"
    );
}

/// The settings: `FAKE` expecting `in_x` as its servers, `OTHER` none, and `github` running
/// `npx`, enabled.
fn x_expects(dir: &Dir, test: &str, in_x: &Value) {
    let x = with_servers(dir, &format!("{test}-x"), in_x);
    let y = with_servers(dir, &format!("{test}-y"), &json!([]));
    dir.set_settings(
        &[(FAKE, x), (OTHER, y)],
        json!({"mcp": {"servers": {"github": {"command": "npx",
            "args": ["-y", "server-github"], "enabled": true}}}}),
    );
}

/// A server scoped to X and given a keychain value through Settings › Tools' `SaveServer`
/// keeps its scope: X's `session/new` carries it with the value, Y's carries none (each
/// scenario checks `mcpServers` exactly).
#[test]
fn a_scoped_server_edited_with_a_new_secret_stays_in_its_matter() {
    let dir = Dir::new("addons_review_edit_secret", &[]);
    let (npx, _, resolver) = programs(&dir);
    let own = vec![json!({"name": "TOKEN", "value": HELD})];
    let server = json!([{"name": "github", "command": npx.display().to_string(),
        "args": ["-y", "server-github"], "env": server_env(&npx, own)}]);
    x_expects(&dir, "review-secret", &server);
    let store = store(&[]);
    let host = dir.start_with(0, |config| wired(config, &store, resolver));
    let (x, y) = two_matters(&dir, &host);
    host.ok(scope(AddonKind::Tool, "github", AddonScope::Matter(x)));
    let edit = local("github", "npx", vec![set("TOKEN", HELD)]);
    host.ok(save(Some("github"), edit));
    still_scoped(&dir, &host, (x, y));
    turn(&host, host.create_thread(x, FAKE));
    turn(&host, host.create_thread(y, OTHER));
    host.shutdown();
}

/// A server scoped to X and given a new program through Settings › Tools' `SaveServer` keeps
/// its scope: X's `session/new` carries the new program, Y's carries none.
#[test]
fn a_scoped_server_edited_with_a_new_command_stays_in_its_matter() {
    let dir = Dir::new("addons_review_edit_command", &[]);
    let (_, uvx, resolver) = programs(&dir);
    let server = json!([{"name": "github", "command": uvx.display().to_string(),
        "args": ["-y", "server-github"], "env": server_env(&uvx, Vec::new())}]);
    x_expects(&dir, "review-command", &server);
    let store = store(&[]);
    let host = dir.start_with(0, |config| wired(config, &store, resolver));
    let (x, y) = two_matters(&dir, &host);
    host.ok(scope(AddonKind::Tool, "github", AddonScope::Matter(x)));
    host.ok(save(Some("github"), local("github", "uvx", Vec::new())));
    still_scoped(&dir, &host, (x, y));
    turn(&host, host.create_thread(x, FAKE));
    turn(&host, host.create_thread(y, OTHER));
    host.shutdown();
}

/// The lawyer's one server `alpha`: the settings that turn it and the browser on, with web
/// search off (so Claude's `allowedTools` holds only `mcp__alpha` and what the session adds),
/// and the entry `session/new` carries for it, as `case_law.rs` has them.
fn alpha(dir: &Dir) -> (Value, Value) {
    let bin = dir.folder("bin", &[]);
    let settings = json!({"tools": {"browser": true, "web_search": false}, "mcp": {"servers": {
        "alpha": {"command": bin.join("alpha").display().to_string(), "enabled": true}}}});
    let entry = json!({"name": "alpha", "command": "{{any}}", "args": [],
        "env": lawyer_env(Vec::new())});
    (settings, entry)
}

/// Writes the settings: `mcp`, and a Claude and a Codex agent each expecting exactly
/// `servers` (as each carries them on this OS), case law's pre-allow and paragraph in
/// Claude's when `with_case_law`. `run` keeps each run's scenarios apart.
fn both_expect(dir: &Dir, run: &str, mcp: &Value, servers: &[Value], with_case_law: bool) {
    let claude_name = format!("review-builtins-claude-{run}");
    let codex_name = format!("review-builtins-codex-{run}");
    let params = claude_params(servers, with_case_law);
    let claude = template(
        dir,
        &claude_name,
        &agent_lines(&claude_name, "session/new", &params, &[]),
    );
    let codex_params = json!({"mcpServers": codex_servers(servers)});
    let codex = template(dir, &codex_name, &codex_lines(&codex_name, &codex_params));
    let entries = agents(&[(CLAUDE, &claude, json!({})), (CODEX, &codex, json!({}))]);
    settings(dir, entries, mcp.clone());
}

/// Whether `seen`'s launch opened an egress proxy: its stderr tail hides the proxy's password
/// only where one opened (`browser_codex.rs`, `case_law.rs`).
fn proxied(seen: &Seen) -> bool {
    seen.redacted
        .iter()
        .any(|secret| secret == TestProxy::PASSWORD)
}

/// `browser` and `case-law` switched off in X: X's Claude and Codex launches carry neither
/// entry (each scenario checks `mcpServers` exactly), write no relay token, open no case-law
/// relay and no proxy, and Claude's header shows no case law. After a restart (the switches are
/// stored), Y's launches carry both, and each opened its relay and its proxy.
#[test]
fn browser_and_case_law_switched_off_in_a_matter_leave_its_launches() {
    let connector = Connector::new(true);
    let dir = Dir::new("addons_review_builtins_off", &[]);
    let (mcp, alpha) = alpha(&dir);
    both_expect(&dir, "x", &mcp, std::slice::from_ref(&alpha), false);
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        wire(config, Some(&connector.relays))
    });
    let (x, y) = two_matters(&dir, &host);
    for id in ["browser", "case-law"] {
        host.ok(set_on(AddonKind::Tool, id, x, false));
    }
    let claude = host.create_thread(x, CLAUDE);
    turn(&host, claude);
    turn(&host, host.create_thread(x, CODEX));
    let tools = first_frame_tools(&host, claude);
    assert!(
        tools.iter().all(|state| state.tool != ClauseoTool::CaseLaw),
        "{tools:?}"
    );
    let seen = launcher.seen();
    assert_eq!(seen.len(), 2);
    for launch in &seen {
        assert_eq!(token_of(launch), None, "a relay token in X");
        assert!(
            !proxied(launch),
            "a proxy opened in X: {:?}",
            launch.redacted
        );
    }
    assert!(
        connector.relays.opened_for_test().is_empty(),
        "a relay opened in X"
    );
    host.shutdown();

    let servers = [alpha, filled_browser(), filled_case_law()];
    both_expect(&dir, "y", &mcp, &servers, true);
    let host = dir.start_launching(1, Arc::<Wired>::clone(&launcher), |config| {
        wire(config, Some(&connector.relays))
    });
    turn(&host, host.create_thread(y, CLAUDE));
    turn(&host, host.create_thread(y, CODEX));
    let seen = launcher.seen();
    assert_eq!(seen.len(), 4);
    for launch in &seen[2..] {
        assert!(token_of(launch).is_some(), "no relay token in Y");
        assert!(proxied(launch), "no proxy in Y");
    }
    let opened = connector.relays.opened_for_test();
    assert_eq!(opened.len(), 2);
    assert!(opened.iter().all(|(key, _)| key.matter == y));
    host.shutdown();
}
