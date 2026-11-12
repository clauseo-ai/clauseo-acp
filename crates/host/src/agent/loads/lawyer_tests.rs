//! The lawyer's servers in one session (`lawyer.rs`): keychain values filled, a server with a
//! value missing left out, the `PATH` and the Windows variables, the Safe workspace, the firm's
//! `managed-mcp.json`, remote servers kept back, and the keychain reads.

#[cfg(unix)]
use std::ffi::OsString;

use clauseo_acp::McpServer;
use clauseo_connectors::{MemoryStore, SecretStore};
use clauseo_settings::{Effective, Locations, SystemFiles};
use serde_json::json;

use super::McpControl;
use super::lawyer::{self, HostEnv, Lawyer, McpSecrets};

/// The settings `servers` make, loaded from a scratch folder.
fn effective(test: &str, servers: &serde_json::Value) -> Effective {
    let root = std::env::temp_dir().join(format!("clauseo-lawyer-{test}-{}", std::process::id()));
    crate::write::create_dir_all(&root).unwrap();
    let text = json!({"mcp": {"servers": servers}}).to_string();
    crate::write::write_file(&root.join("settings.json"), text.as_bytes()).unwrap();
    let effective = Effective::load(&Locations::under_root(&root), &SystemFiles, &[]);
    crate::write::remove_dir_all(&root).unwrap();
    assert_eq!(effective.warnings(), []);
    effective
}

/// `path`, written `/a/b`, as an absolute path on this OS: `C:\a\b` on Windows.
fn abs(path: &str) -> String {
    if cfg!(windows) {
        format!("C:{}", path.replace('/', "\\"))
    } else {
        path.to_owned()
    }
}

const CLAUSEO: Lawyer = Lawyer {
    control: McpControl::Clauseo,
    safe_workspace: false,
};

#[cfg(unix)]
fn env_of(server: &McpServer) -> Vec<(String, String)> {
    let McpServer::Stdio { env, .. } = server else {
        panic!("a lawyer server goes as stdio: {server:?}");
    };
    env.clone()
}

#[cfg(unix)]
#[test]
fn values_are_filled_and_the_path_starts_with_the_commands_folder() {
    let effective = effective(
        "filled",
        &json!({
            "github": {"command": "/opt/homebrew/bin/npx", "args": ["-y", "x"], "enabled": true,
                "env": {"TOKEN": {"keychain": true}, "REGION": "uk"}},
            "own-path": {"command": "/tools/srv", "enabled": true, "env": {"PATH": "/mine"}},
            "off": {"command": "/tools/off"},
            "remote": {"url": "https://x.example/mcp", "enabled": true},
        }),
    );
    let secrets = McpSecrets::of(
        &[("github/env/TOKEN", "t0ken")],
        &[
            ("/opt/homebrew/bin/npx", "/opt/homebrew/bin/npx"),
            ("/tools/srv", "/tools/srv"),
        ],
    );
    let host_path = OsString::from("/usr/bin:/bin");
    let host = HostEnv {
        path: Some(("PATH", &host_path)),
        windows_env: &[("SystemRoot".to_owned(), "C:\\Windows".to_owned())],
    };
    let (servers, left_out) = lawyer::servers(&effective, CLAUSEO, &secrets, host);
    assert_eq!(left_out, Vec::new());
    assert_eq!(servers.len(), 2, "{servers:?}");
    assert_eq!(
        env_of(&servers[0]),
        [
            ("REGION".to_owned(), "uk".to_owned()),
            ("TOKEN".to_owned(), "t0ken".to_owned()),
            (
                "PATH".to_owned(),
                "/opt/homebrew/bin:/usr/bin:/bin".to_owned()
            ),
            ("SystemRoot".to_owned(), "C:\\Windows".to_owned()),
        ]
    );
    assert_eq!(
        env_of(&servers[1]),
        [
            ("PATH".to_owned(), "/mine".to_owned()),
            ("SystemRoot".to_owned(), "C:\\Windows".to_owned()),
        ]
    );
}

#[test]
fn a_server_without_its_value_is_left_out_and_named() {
    let (npx, plain) = (abs("/opt/homebrew/bin/npx"), abs("/opt/homebrew/bin/plain"));
    let effective = effective(
        "missing",
        &json!({
            "github": {"command": npx, "enabled": true,
                "env": {"TOKEN": {"keychain": true}}},
            "plain": {"command": "plain", "enabled": true},
            "gone": {"command": "not-here", "enabled": true},
        }),
    );
    let host = HostEnv {
        path: None,
        windows_env: &[],
    };
    let secrets = McpSecrets::of(
        &[],
        &[(npx.as_str(), npx.as_str()), ("plain", plain.as_str())],
    )
    .with_unreadable(&["github/env/TOKEN"]);
    let (servers, left_out) = lawyer::servers(&effective, CLAUSEO, &secrets, host);
    let names: Vec<&str> = servers.iter().map(McpServer::name).collect();
    assert_eq!(names, ["plain"]);
    let McpServer::Stdio { command, env, .. } = &servers[0] else {
        panic!("a lawyer server goes as stdio: {:?}", servers[0]);
    };
    assert_eq!(command, std::path::Path::new(&plain));
    // The resolved program's folder leads its PATH.
    let folder = abs("/opt/homebrew/bin");
    assert!(
        env.iter()
            .any(|(name, value)| name == "PATH" && value.starts_with(&folder))
    );
    assert_eq!(left_out, [("github".to_owned(), "TOKEN".to_owned())]);
}

#[test]
fn none_go_in_the_safe_workspace_or_under_managed_mcp() {
    let plain = abs("/opt/homebrew/bin/plain");
    let effective = effective(
        "fenced",
        &json!({"plain": {"command": plain, "enabled": true,
            "env": {"TOKEN": {"keychain": true}}}}),
    );
    let host = HostEnv {
        path: None,
        windows_env: &[],
    };
    let secrets = McpSecrets::of(
        &[("plain/env/TOKEN", "t")],
        &[(plain.as_str(), plain.as_str())],
    );
    for lawyer in [
        Lawyer {
            control: McpControl::Clauseo,
            safe_workspace: true,
        },
        Lawyer {
            control: McpControl::Managed,
            safe_workspace: false,
        },
    ] {
        assert_eq!(
            lawyer::servers(&effective, lawyer, &secrets, host),
            (Vec::new(), Vec::new())
        );
    }
    assert_eq!(lawyer::wanted(&effective, true), lawyer::Wanted::default());
    assert_eq!(
        lawyer::wanted(&effective, false),
        lawyer::Wanted {
            accounts: vec!["plain/env/TOKEN".to_owned()],
            commands: vec![plain],
        }
    );
}

#[test]
fn reads_keep_only_usable_values() {
    let store = MemoryStore::new();
    store.put("s/env/OK", b"s3cr3t");
    store.put("s/env/NUL", b"va\0lue");
    store.put("s/env/BYTES", &[0xff, 0xfe]);
    let accounts = ["s/env/OK", "s/env/NUL", "s/env/BYTES", "s/env/GONE"]
        .map(str::to_owned)
        .to_vec();
    let resolve: lawyer::Resolve =
        std::sync::Arc::new(|typed: &str| (typed == "npx").then(|| "/bin/npx".into()));
    let wanted = lawyer::Wanted {
        accounts,
        commands: vec!["npx".to_owned(), "gone".to_owned()],
    };
    let secrets = lawyer::read(Some(&store as &dyn SecretStore), &resolve, wanted.clone());
    let debug = format!("{secrets:?}");
    assert_eq!(
        debug,
        "McpSecrets { values: 1, unreadable: 3, commands: 1 }"
    );
    assert!(!debug.contains("s3cr3t"));
    let none = lawyer::read(None, &resolve, wanted);
    assert_eq!(
        format!("{none:?}"),
        "McpSecrets { values: 0, unreadable: 4, commands: 1 }"
    );
}

/// Every value an enabled local server carries is hidden in the agent's stderr tail: the
/// keychain values read and each plain variable value; a disabled server's are not needed.
#[test]
fn every_value_of_an_enabled_server_is_redacted() {
    let effective = effective(
        "redacted",
        &json!({
            "github": {"command": abs("/opt/homebrew/bin/npx"), "enabled": true,
                "env": {"TOKEN": {"keychain": true}, "PLAIN": "plain-value-0042"}},
            "remote": {"url": "https://x.example/mcp", "enabled": true,
                "headers": {"X-Key": "header-value-0042"}},
            "off": {"command": abs("/opt/off"), "env": {"OFF": "off-value-0042"}},
        }),
    );
    let secrets = McpSecrets::of(&[("github/env/TOKEN", "keychain-value-0042")], &[]);
    let mut redacted = lawyer::redacted(&effective, &secrets);
    redacted.sort();
    // A remote server's values are PR B's to add.
    assert_eq!(redacted, ["keychain-value-0042", "plain-value-0042"]);
}

/// A server renamed after the launch prep read its values is left out of that session, but not
/// marked missing: its new account was never read, so nothing is known about it.
#[test]
fn a_server_renamed_during_a_launch_is_left_out_but_not_marked_missing() {
    let npx = abs("/opt/homebrew/bin/npx");
    let effective = effective(
        "renamed",
        &json!({"github": {"command": npx, "enabled": true,
            "env": {"TOKEN": {"keychain": true}}}}),
    );
    // The prep read the old name's item.
    let secrets = McpSecrets::of(
        &[("gh/env/TOKEN", "t0ken-value")],
        &[(npx.as_str(), npx.as_str())],
    );
    let host = HostEnv {
        path: None,
        windows_env: &[],
    };
    let (servers, left_out) = lawyer::servers(&effective, CLAUSEO, &secrets, host);
    assert_eq!(servers, Vec::new());
    assert_eq!(left_out, Vec::new());
}

/// The server's `PATH` holds its program's folder and the host's absolute folders only: a
/// relative or empty entry, which would resolve against the matter folder, is left out.
#[cfg(unix)]
#[test]
fn the_path_holds_no_relative_folder() {
    let effective = effective(
        "relative",
        &json!({"plain": {"command": "/opt/homebrew/bin/plain", "enabled": true}}),
    );
    let secrets = McpSecrets::of(
        &[],
        &[("/opt/homebrew/bin/plain", "/opt/homebrew/bin/plain")],
    );
    let host_path = OsString::from("/usr/bin:.:bin::/bin");
    let host = HostEnv {
        path: Some(("PATH", &host_path)),
        windows_env: &[],
    };
    let (servers, _) = lawyer::servers(&effective, CLAUSEO, &secrets, host);
    assert_eq!(
        env_of(&servers[0]),
        [(
            "PATH".to_owned(),
            "/opt/homebrew/bin:/usr/bin:/bin".to_owned()
        )]
    );
}
