//! The lawyer's servers in one session (`lawyer.rs`): keychain values filled, a server with a
//! value missing left out, the `PATH` and the Windows variables, the Safe workspace, the firm's
//! `managed-mcp.json`, remote servers with their headers, and the keychain reads.

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
        panic!("expected a stdio server: {server:?}");
    };
    env.clone()
}

const NO_HOST: HostEnv<'static> = HostEnv {
    path: None,
    windows_env: &[],
};

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
    assert_eq!(servers.len(), 3, "{servers:?}");
    assert_eq!(
        servers[2],
        McpServer::Http {
            name: "remote".to_owned(),
            url: "https://x.example/mcp".to_owned(),
            headers: Vec::new(),
        }
    );
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
        panic!("expected a stdio server");
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

/// Every value an enabled server carries is hidden in the agent's stderr tail: the keychain
/// values read and each plain variable or header value; a disabled server's are not needed.
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
    let redacted = lawyer::redacted(&effective, &secrets);
    for value in [
        "keychain-value-0042",
        "plain-value-0042",
        "header-value-0042",
    ] {
        assert!(
            redacted.iter().any(|hidden| hidden == value),
            "{value}: {redacted:?}"
        );
    }
    assert!(
        !redacted.iter().any(|hidden| hidden.contains("off-value")),
        "{redacted:?}"
    );
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

/// A remote server's headers: a keychain value read through the store (A1's in-memory double),
/// a plain one as it is, in name order. One whose keychain value is missing, or holds a CR or
/// LF, is left out and named; a server with none goes with no headers.
#[test]
fn a_remote_server_goes_with_its_headers_filled_from_the_keychain() {
    let effective = effective(
        "remote",
        &json!({
            "docs": {"url": "https://mcp.example.com/mcp", "enabled": true, "headers": {
                "X-Tenant": "firm-7", "Authorization": {"keychain": true}}},
            "gone": {"url": "https://gone.example.com/mcp", "enabled": true, "headers": {
                "Authorization": {"keychain": true}}},
            "split": {"url": "https://split.example.com/mcp", "enabled": true, "headers": {
                "Authorization": {"keychain": true}}},
            "open": {"url": "http://localhost:8080/mcp", "enabled": true},
            "off": {"url": "https://off.example.com/mcp", "headers": {
                "Authorization": {"keychain": true}}},
        }),
    );
    let wanted = lawyer::wanted(&effective, false);
    assert_eq!(
        wanted.accounts,
        [
            "docs/header/Authorization",
            "gone/header/Authorization",
            "split/header/Authorization",
        ]
    );
    assert_eq!(wanted.commands, Vec::<String>::new());
    let store = MemoryStore::new();
    store.put("docs/header/Authorization", b"Bearer s3cret-header");
    store.put("split/header/Authorization", b"Bearer x\r\nX-Evil: 1");
    let resolve: lawyer::Resolve = std::sync::Arc::new(|_: &str| None);
    let secrets = lawyer::read(Some(&store as &dyn SecretStore), &resolve, wanted);
    let (servers, left_out) = lawyer::servers(&effective, CLAUSEO, &secrets, NO_HOST);
    assert_eq!(
        servers,
        [
            McpServer::Http {
                name: "docs".to_owned(),
                url: "https://mcp.example.com/mcp".to_owned(),
                headers: vec![
                    (
                        "Authorization".to_owned(),
                        "Bearer s3cret-header".to_owned()
                    ),
                    ("X-Tenant".to_owned(), "firm-7".to_owned()),
                ],
            },
            McpServer::Http {
                name: "open".to_owned(),
                url: "http://localhost:8080/mcp".to_owned(),
                headers: Vec::new(),
            },
        ]
    );
    assert_eq!(
        left_out,
        [
            ("gone".to_owned(), "Authorization".to_owned()),
            ("split".to_owned(), "Authorization".to_owned()),
        ]
    );
}

/// No remote server goes in the Safe workspace or under the firm's `managed-mcp.json`, and
/// nothing is read for one in the Safe workspace.
#[test]
fn no_remote_server_goes_in_the_safe_workspace_or_under_managed_mcp() {
    let effective = effective(
        "remote-fenced",
        &json!({"docs": {"url": "https://mcp.example.com/mcp", "enabled": true,
            "headers": {"Authorization": {"keychain": true}}}}),
    );
    let secrets = McpSecrets::of(&[("docs/header/Authorization", "Bearer t0ken-value")], &[]);
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
            lawyer::servers(&effective, lawyer, &secrets, NO_HOST),
            (Vec::new(), Vec::new())
        );
    }
    assert_eq!(lawyer::wanted(&effective, true), lawyer::Wanted::default());
}

/// What the stderr redactor gets, in the forms an error line prints them in: each keychain
/// value and plain header value as it is, JSON-escaped, percent-encoded and after its scheme
/// (Basic: decoded, and the password); the URL whole, its host, its path with its query, its
/// query, each query value (decoded too) and each long path segment. A value under 6 bytes is
/// left out (and logged by name).
#[test]
fn the_redacted_list_holds_each_value_and_url_part_in_its_forms() {
    let effective = effective(
        "redacted",
        &json!({
            "docs": {"url": "https://mcp.example.com:8443/v1/k3yLikeSegment0123/mcp?key=q7%2Btoken&n=1",
                "enabled": true, "headers": {"Authorization": {"keychain": true},
                    "X-Tenant": "firm\"tenant/7", "X-Short": "abc"}},
            "local": {"command": "/tools/srv", "enabled": true, "env": {"TOKEN": {"keychain": true}}},
        }),
    );
    let secrets = McpSecrets::of(
        &[
            // user1:password1
            ("docs/header/Authorization", "Basic dXNlcjE6cGFzc3dvcmQx"),
            ("local/env/TOKEN", "env-t0ken-value"),
        ],
        &[],
    );
    let redacted = lawyer::redacted(&effective, &secrets);
    for form in [
        "Basic dXNlcjE6cGFzc3dvcmQx",
        "dXNlcjE6cGFzc3dvcmQx",
        "user1:password1",
        "password1",
        "env-t0ken-value",
        "firm\"tenant/7",
        "firm\\\"tenant/7",
        "firm%22tenant%2F7",
        "firm%22tenant%2f7",
        "https://mcp.example.com:8443/v1/k3yLikeSegment0123/mcp?key=q7%2Btoken&n=1",
        "https://mcp.example.com:8443/v1/k3yLikeSegment0123/mcp?key=q7+token&n=1",
        "mcp.example.com:8443",
        "mcp.example.com",
        "/v1/k3yLikeSegment0123/mcp?key=q7%2Btoken&n=1",
        "key=q7%2Btoken&n=1",
        "q7%2Btoken",
        "q7+token",
        "k3yLikeSegment0123",
    ] {
        assert!(
            redacted.iter().any(|hidden| hidden == form),
            "{form} not in {redacted:?}"
        );
    }
    for short in ["abc", "1", "/mcp", "v1"] {
        assert!(
            !redacted.iter().any(|hidden| hidden == short),
            "{short} in {redacted:?}"
        );
    }
    assert!(
        redacted.iter().all(|hidden| hidden.len() >= 6),
        "{redacted:?}"
    );
}

/// A header value holding CR, LF, NUL or DEL, plain (a firm's policy, a hand-edited file) or
/// from the keychain, never reaches the wire: its server is left out, named with the header.
#[test]
fn a_header_value_with_a_control_character_never_goes() {
    for (n, bad) in ["a\rb", "a\nX-Evil: 1", "a\u{0}b", "a\u{7f}b"]
        .iter()
        .enumerate()
    {
        let effective = effective(
            &format!("control-{n}"),
            &json!({
                "plain": {"url": "https://plain.example.com/mcp", "enabled": true,
                    "headers": {"X-Api-Key": bad}},
                "kept": {"url": "https://kept.example.com/mcp", "enabled": true,
                    "headers": {"X-Api-Key": {"keychain": true}}},
                "ok": {"url": "https://ok.example.com/mcp", "enabled": true,
                    "headers": {"X-Api-Key": "fine\tvalue"}},
            }),
        );
        let secrets = McpSecrets::of(&[("kept/header/X-Api-Key", bad)], &[]);
        let (servers, left_out) = lawyer::servers(&effective, CLAUSEO, &secrets, NO_HOST);
        assert_eq!(
            servers,
            [McpServer::Http {
                name: "ok".to_owned(),
                url: "https://ok.example.com/mcp".to_owned(),
                headers: vec![("X-Api-Key".to_owned(), "fine\tvalue".to_owned())],
            }],
            "{bad:?}"
        );
        assert_eq!(
            left_out,
            [
                ("kept".to_owned(), "X-Api-Key".to_owned()),
                ("plain".to_owned(), "X-Api-Key".to_owned()),
            ],
            "{bad:?}"
        );
    }
}

#[test]
fn a_header_value_holds_no_control_character_but_tab() {
    assert!(lawyer::header_value_ok("Bearer abc\tdef"));
    assert!(lawyer::header_value_ok("r\u{e9}sum\u{e9}"));
    for bad in ["a\rb", "a\nb", "a\0b", "a\u{7f}b"] {
        assert!(!lawyer::header_value_ok(bad), "{bad:?}");
    }
}
