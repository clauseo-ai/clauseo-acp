use std::collections::BTreeMap;
use std::ffi::OsString;

use clauseo_acp::McpServer;
use clauseo_settings::{Effective, Locations, SystemFiles};
use serde_json::{Map, Value, json};

use super::{
    CLAUDE_AI_MCP_VAR, CLAUDE_MANAGED_SETTING_SOURCES, CLAUDE_SETTING_SOURCES, GUARD_MATCHER,
    GuardHook, McpControl, guard_entry, launch_env, mcp_control, mcp_servers, session_meta,
};
use crate::write;

fn object(value: &Value) -> Map<String, Value> {
    let Some(map) = value.as_object() else {
        panic!("not an object: {value}");
    };
    map.clone()
}

#[test]
fn with_no_hook_the_meta_is_the_switches() {
    let meta = session_meta(None, McpControl::Clauseo, None, None);
    assert_eq!(
        Value::Object(meta),
        json!({"claudeCode": {"options": {
            "strictMcpConfig": true,
            "settingSources": CLAUDE_SETTING_SOURCES,
            "env": {"ENABLE_CLAUDEAI_MCP_SERVERS": "false"},
        }}})
    );
    assert_eq!(CLAUDE_SETTING_SOURCES, ["user"]);
}

#[test]
fn the_switches_win_over_the_hook_and_every_other_hook_key_stays() {
    let hook = object(&json!({
        "clauseo_test": true,
        "claudeCode": {
            "other": 1,
            "options": {
                "strictMcpConfig": false,
                "settingSources": ["user", "project", "local"],
                "model": "opus",
                "settings": {"syncClaudeAiPlugins": false},
                "env": {"ENABLE_CLAUDEAI_MCP_SERVERS": "true", "FIRM": "1"},
            },
        },
    }));
    let meta = session_meta(Some(hook), McpControl::Clauseo, None, None);
    assert_eq!(
        Value::Object(meta),
        json!({
            "clauseo_test": true,
            "claudeCode": {
                "other": 1,
                "options": {
                    "strictMcpConfig": true,
                    "settingSources": ["user"],
                    "model": "opus",
                    "settings": {"syncClaudeAiPlugins": false},
                    "env": {"ENABLE_CLAUDEAI_MCP_SERVERS": "false", "FIRM": "1"},
                },
            },
        })
    );
}

#[test]
fn a_hook_value_that_is_not_an_object_is_replaced() {
    for hook in [
        json!({"claudeCode": "off"}),
        json!({"claudeCode": {"options": []}}),
        json!({"claudeCode": {"options": {"env": null}}}),
    ] {
        let meta = Value::Object(session_meta(
            Some(object(&hook)),
            McpControl::Clauseo,
            None,
            None,
        ));
        let options = &meta["claudeCode"]["options"];
        assert_eq!(options["strictMcpConfig"], true, "{hook}");
        assert_eq!(options["env"][CLAUDE_AI_MCP_VAR], "false", "{hook}");
    }
}

/// The guard's entry: the exec form with `--matter <folder>`, the matcher and the timeout,
/// appended after the hook's own entries, with `disableAllHooks: false` beside them; a hook value
/// of another type at `settings`, `hooks` or `PreToolUse` is replaced, and a hook's
/// `disableAllHooks: true` is overridden.
#[test]
fn the_guard_entry_is_appended_after_the_hooks_entries() {
    let program = std::env::temp_dir()
        .join("Clauseo O'Brien")
        .join("clauseo-desktop");
    let matter = std::env::temp_dir().join("Matters").join("Smith v Jones");
    let guard = Some(GuardHook {
        program: &program,
        matter: &matter,
    });
    let entry = json!({
        "matcher": "Edit|Write|MultiEdit|NotebookEdit|Bash|PowerShell",
        "hooks": [{
            "type": "command",
            "command": program.to_str().unwrap(),
            "args": ["--pre-write-guard", "--matter", matter.to_str().unwrap()],
            "timeout": 10,
        }],
    });
    assert_eq!(
        GUARD_MATCHER,
        "Edit|Write|MultiEdit|NotebookEdit|Bash|PowerShell"
    );
    assert_eq!(guard_entry(&program, &matter), Some(entry.clone()));
    let firm = json!({"matcher": "Read", "hooks": [{"type": "command", "command": "firm"}]});
    let hook = object(&json!({"claudeCode": {"options": {"settings": {
        "syncClaudeAiPlugins": false,
        "disableAllHooks": true,
        "hooks": {"PreToolUse": [firm], "Stop": []},
    }}}}));
    let meta = Value::Object(session_meta(Some(hook), McpControl::Clauseo, guard, None));
    assert_eq!(
        meta["claudeCode"]["options"]["settings"],
        json!({
            "syncClaudeAiPlugins": false,
            "disableAllHooks": false,
            "hooks": {"PreToolUse": [firm, entry], "Stop": []},
        })
    );
    for hook in [
        json!({"claudeCode": {"options": {"settings": "none"}}}),
        json!({"claudeCode": {"options": {"settings": {"hooks": []}}}}),
        json!({"claudeCode": {"options": {"settings": {"hooks": {"PreToolUse": {}}}}}}),
    ] {
        let meta = Value::Object(session_meta(
            Some(object(&hook)),
            McpControl::Managed,
            guard,
            None,
        ));
        assert_eq!(
            meta["claudeCode"]["options"]["settings"],
            json!({"disableAllHooks": false, "hooks": {"PreToolUse": [entry]}}),
            "{hook}"
        );
    }
    let meta = Value::Object(session_meta(None, McpControl::Clauseo, None, None));
    assert_eq!(meta["claudeCode"]["options"].get("settings"), None);
}

/// A program or matter path that is not UTF-8 cannot go in JSON: no entry, rather than a lossy
/// one.
#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_gets_no_entry() {
    use std::os::unix::ffi::OsStrExt;
    let odd = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/opt/clauseo-\xff"));
    let plain = std::path::Path::new("/opt/clauseo");
    assert_eq!(guard_entry(odd, plain), None);
    assert_eq!(guard_entry(plain, odd), None);
    let guard = Some(GuardHook {
        program: odd,
        matter: plain,
    });
    let meta = Value::Object(session_meta(None, McpControl::Clauseo, guard, None));
    assert_eq!(meta["claudeCode"]["options"].get("settings"), None);
}

#[test]
fn under_managed_mcp_strict_is_off_even_when_the_hook_asks() {
    let hook = object(&json!({"claudeCode": {"options": {"strictMcpConfig": true}}}));
    let meta = Value::Object(session_meta(Some(hook), McpControl::Managed, None, None));
    let options = &meta["claudeCode"]["options"];
    assert_eq!(options["strictMcpConfig"], false);
    assert_eq!(
        options["settingSources"],
        json!(CLAUDE_MANAGED_SETTING_SOURCES)
    );
    assert_eq!(options["env"][CLAUDE_AI_MCP_VAR], "false");
}

// The hook cannot widen the sources through an extra flag (clauseo-acp-private#185): the SDK
// writes extraArgs after its own --setting-sources, so a later one would win.
#[test]
fn the_hooks_setting_sources_flag_is_taken_out_of_extra_args() {
    let hook = object(&json!({"claudeCode": {"options": {"extraArgs": {
        "setting-sources": "user,project,local",
        "setting-sources=project": null,
        "setting-sourcesx": "kept",
        "debug": null,
    }}}}));
    for control in [McpControl::Clauseo, McpControl::Managed] {
        let meta = Value::Object(session_meta(Some(hook.clone()), control, None, None));
        let options = &meta["claudeCode"]["options"];
        assert_eq!(
            options["extraArgs"],
            json!({"setting-sourcesx": "kept", "debug": null}),
            "{control:?}"
        );
        assert_eq!(options["settingSources"], json!(["user"]), "{control:?}");
    }
    // extraArgs that is not an object is left as the hook set it.
    let hook = object(&json!({"claudeCode": {"options": {"extraArgs": "x"}}}));
    let meta = Value::Object(session_meta(Some(hook), McpControl::Clauseo, None, None));
    assert_eq!(meta["claudeCode"]["options"]["extraArgs"], "x");
}

#[test]
fn under_managed_mcp_the_hooks_mcp_flags_are_taken_out_too() {
    let hook = object(&json!({"claudeCode": {"options": {"extraArgs": {
        "mcp-config": "/tmp/servers.json",
        "strict-mcp-config": null,
        "strict-mcp-config=true": null,
        "mcp-configx": "kept",
    }}}}));
    let clauseo = Value::Object(session_meta(
        Some(hook.clone()),
        McpControl::Clauseo,
        None,
        None,
    ));
    assert_eq!(
        clauseo["claudeCode"]["options"]["extraArgs"],
        json!({
            "mcp-config": "/tmp/servers.json",
            "strict-mcp-config": null,
            "strict-mcp-config=true": null,
            "mcp-configx": "kept",
        })
    );
    let managed = Value::Object(session_meta(Some(hook), McpControl::Managed, None, None));
    assert_eq!(
        managed["claudeCode"]["options"]["extraArgs"],
        json!({"mcp-configx": "kept"})
    );
}

/// R21: a hook cannot add servers through `_meta`, under Clauseo control as under Managed.
#[test]
fn the_hooks_meta_servers_go_in_every_mode() {
    let hook = object(&json!({"claudeCode": {"options": {
        "mcpServers": {"clauseo": {"command": "/opt/clauseo-mcp"}},
    }}}));
    let clauseo = Value::Object(session_meta(
        Some(hook.clone()),
        McpControl::Clauseo,
        None,
        None,
    ));
    assert_eq!(clauseo["claudeCode"]["options"].get("mcpServers"), None);
    let managed = Value::Object(session_meta(Some(hook), McpControl::Managed, None, None));
    assert_eq!(managed["claudeCode"]["options"].get("mcpServers"), None);
}

#[test]
fn the_launch_variable_is_set_after_the_hooks_in_any_case() {
    let hook = BTreeMap::from([
        (
            OsString::from("enable_claudeai_mcp_servers"),
            OsString::from("true"),
        ),
        (OsString::from("FIRM"), OsString::from("1")),
    ]);
    assert_eq!(
        launch_env(hook),
        BTreeMap::from([
            (OsString::from(CLAUDE_AI_MCP_VAR), OsString::from("false")),
            (OsString::from("FIRM"), OsString::from("1")),
        ])
    );
    assert_eq!(
        launch_env(BTreeMap::new()),
        BTreeMap::from([(OsString::from(CLAUDE_AI_MCP_VAR), OsString::from("false"))])
    );
}

/// A folder of the test's own under the temp folder, removed after.
fn scratch(test: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("clauseo-host-loads-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

#[test]
fn managed_mcp_is_a_file_at_the_path() {
    let root = scratch("managed");
    let path = root.join("managed-mcp.json");
    assert_eq!(mcp_control(&path), McpControl::Clauseo);
    write::write_file(&path, b"not even json").unwrap();
    assert_eq!(mcp_control(&path), McpControl::Managed);
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn only_enabled_servers_go_in_name_order_and_none_under_managed_mcp() {
    let root = scratch("servers");
    let bin = |name: &str| root.join("bin").join(name).display().to_string();
    let settings = json!({"mcp": {"servers": {
        "zeta": {"command": bin("zeta"), "enabled": true},
        "beta": {"command": bin("beta")},
        "alpha": {"command": bin("alpha"), "args": ["--stdio"], "env": {"B": "2", "A": "1"},
            "enabled": true},
    }}});
    write::write_file(&root.join("settings.json"), settings.to_string().as_bytes()).unwrap();
    let effective = Effective::load(&Locations::under_root(&root), &SystemFiles, &[]);
    assert_eq!(effective.warnings(), []);
    let servers = mcp_servers(&effective, McpControl::Clauseo);
    assert_eq!(
        servers,
        vec![
            McpServer::Stdio {
                name: "alpha".to_owned(),
                command: bin("alpha").into(),
                args: vec!["--stdio".to_owned()],
                env: vec![
                    ("A".to_owned(), "1".to_owned()),
                    ("B".to_owned(), "2".to_owned())
                ],
            },
            McpServer::Stdio {
                name: "zeta".to_owned(),
                command: bin("zeta").into(),
                args: Vec::new(),
                env: Vec::new(),
            },
        ]
    );
    assert_eq!(mcp_servers(&effective, McpControl::Managed), Vec::new());
    write::remove_dir_all(&root).unwrap();
}
