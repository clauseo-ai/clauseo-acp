//! Import from each app's config: Claude Desktop, Claude Code, Codex and Windows lookups.

use super::*;

#[test]
fn mac_claude_desktop_local_and_remote() {
    let fs = FakeFs::default().program("/usr/local/bin/mcp-files").file(
        DESKTOP_MAC,
        r#"{"mcpServers": {
                "files": {"command": "/usr/local/bin/mcp-files",
                          "args": ["--root", "${HOME}/x"],
                          "env": {"API_KEY": "SECRET-desktop-1"}},
                "web": {"url": "https://web.example.com/mcp",
                        "headers": {"Authorization": "Bearer SECRET-header-1"}}
            }}"#,
    );
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(names(&found), ["files", "web"]);

    let files = by_name(&found, "files");
    assert_eq!(files.sources, [Source::ClaudeDesktop]);
    assert_eq!(
        files.kind,
        local(
            "/usr/local/bin/mcp-files",
            &["--root", "${HOME}/x"],
            &[("API_KEY", "SECRET-desktop-1")]
        )
    );
    // Claude Desktop's text is kept as written: no expansion.
    assert_eq!(files.problems.len(), 0, "{:?}", files.problems);

    let web = by_name(&found, "web");
    assert_eq!(
        web.kind,
        remote(
            "https://web.example.com/mcp",
            &[("Authorization", "Bearer SECRET-header-1")]
        )
    );
    assert_eq!(web.problems.len(), 0);
}

#[test]
fn windows_claude_desktop_uses_appdata_and_pathext() {
    let fs = FakeFs::default()
        .file(
            DESKTOP_WIN,
            r#"{"mcpServers": {
                "ddg": {"command": "npx", "args": ["-y", "duck"]},
                "shell": {"command": "cmd"},
                "uv": {"command": "uvx"},
                "abs": {"command": "C:\\Program Files\\nodejs\\npx"}
            }}"#,
        )
        // Mac's config must not be read on Windows.
        .file(
            DESKTOP_MAC,
            r#"{"mcpServers": {"wrong": {"command": "/bin/x"}}}"#,
        )
        .program("C:\\Users\\ana\\AppData\\Roaming\\npm\\npx.cmd")
        .program("C:\\Windows\\System32\\cmd.exe")
        .program("C:\\tools\\uvx.exe")
        .program("C:\\Program Files\\nodejs\\npx.cmd");
    let pairs = [
        ("APPDATA", "C:\\Users\\ana\\AppData\\Roaming"),
        ("SystemRoot", "C:\\Windows"),
        ("PATH", "C:\\tools;C:\\Windows\\System32"),
    ];
    let found = run(Os::Windows, "C:\\Users\\ana", &pairs, &fs, &[]);
    assert_eq!(names(&found), ["abs", "ddg", "shell", "uv"]);

    assert_eq!(
        by_name(&found, "ddg").kind,
        local(
            "C:\\Users\\ana\\AppData\\Roaming\\npm\\npx.cmd",
            &["-y", "duck"],
            &[]
        )
    );
    assert_eq!(
        by_name(&found, "shell").kind,
        local("C:\\Windows\\System32\\cmd.exe", &[], &[])
    );
    assert_eq!(
        by_name(&found, "uv").kind,
        local("C:\\tools\\uvx.exe", &[], &[])
    );
    assert_eq!(
        by_name(&found, "abs").kind,
        local("C:\\Program Files\\nodejs\\npx.cmd", &[], &[])
    );
    for candidate in &found {
        assert_eq!(candidate.problems.len(), 0, "{candidate:?}");
    }
}

#[test]
fn windows_command_not_found_keeps_the_command_as_given() {
    let fs = FakeFs::default().file(
        DESKTOP_WIN,
        r#"{"mcpServers": {"gone": {"command": "npx"}}}"#,
    );
    let pairs = [("APPDATA", "C:\\Users\\ana\\AppData\\Roaming")];
    let found = run(Os::Windows, "C:\\Users\\ana", &pairs, &fs, &[]);
    let gone = by_name(&found, "gone");
    assert_eq!(gone.kind, local("npx", &[], &[]));
    assert_eq!(
        gone.problems,
        [Problem::CommandNotFound {
            original: "npx".to_owned()
        }]
    );
}

#[test]
fn windows_without_appdata_reads_no_desktop_file() {
    let fs = FakeFs::default().file(
        DESKTOP_WIN,
        r#"{"mcpServers": {"x": {"command": "/bin/x"}}}"#,
    );
    assert_eq!(run(Os::Windows, "C:\\Users\\ana", &[], &fs, &[]).len(), 0);
}

#[test]
fn mac_search_path_and_nvm_newest_version() {
    let fs = FakeFs::default()
        .file(
            "/Users/ana/.claude.json",
            r#"{"mcpServers": {
                "npm-tool": {"command": "npx", "args": ["-y", "pkg"]},
                "uv-tool": {"command": "uvx"},
                "git": {"command": "git"},
                "own": {"command": "tool"},
                "missing": {"command": "nope-bin"},
                "abs": {"command": "/nope/tool"}
            }}"#,
        )
        .program("/Users/ana/.nvm/versions/node/v9.11.2/bin/npx")
        .program("/Users/ana/.nvm/versions/node/v20.1.0/bin/npx")
        .program("/Users/ana/.nvm/versions/node/v18.19.1/bin/npx")
        .dir(
            "/Users/ana/.nvm/versions/node",
            &[
                "/Users/ana/.nvm/versions/node/v9.11.2",
                "/Users/ana/.nvm/versions/node/v20.1.0",
                "/Users/ana/.nvm/versions/node/v18.19.1",
                "/Users/ana/.nvm/versions/node/notes.txt",
            ],
        )
        .program("/Users/ana/.local/bin/uvx")
        .program("/opt/homebrew/bin/git")
        .program("/usr/local/bin/git")
        .program("/Users/ana/bin/tool");
    let pairs = [("PATH", "/Users/ana/bin:/usr/bin:/bin")];
    let found = run(Os::Mac, "/Users/ana", &pairs, &fs, &[]);
    assert_eq!(
        names(&found),
        ["abs", "git", "missing", "npm-tool", "own", "uv-tool"]
    );

    assert_eq!(
        by_name(&found, "npm-tool").kind,
        local(
            "/Users/ana/.nvm/versions/node/v20.1.0/bin/npx",
            &["-y", "pkg"],
            &[]
        )
    );
    assert_eq!(
        by_name(&found, "uv-tool").kind,
        local("/Users/ana/.local/bin/uvx", &[], &[])
    );
    assert_eq!(
        by_name(&found, "git").kind,
        local("/opt/homebrew/bin/git", &[], &[])
    );
    assert_eq!(
        by_name(&found, "own").kind,
        local("/Users/ana/bin/tool", &[], &[])
    );

    let missing = by_name(&found, "missing");
    assert_eq!(missing.kind, local("nope-bin", &[], &[]));
    assert_eq!(
        missing.problems,
        [Problem::CommandNotFound {
            original: "nope-bin".to_owned()
        }]
    );
    let abs = by_name(&found, "abs");
    assert_eq!(abs.kind, local("/nope/tool", &[], &[]));
    assert_eq!(
        abs.problems,
        [Problem::CommandNotFound {
            original: "/nope/tool".to_owned()
        }]
    );
}

#[test]
fn claude_code_user_project_and_mcp_json() {
    let fs = FakeFs::default()
        .file(
            "/Users/ana/.claude.json",
            r#"{
                "mcpServers": {
                    "notes": {"type": "stdio", "command": "/opt/notes", "args": [],
                              "env": {"TOKEN": "${NOTES_TOKEN}"}}
                },
                "projects": {
                    "/Users/ana/Matters/Smith": {"mcpServers": {
                        "case-tool": {"type": "http",
                                      "url": "https://tool.example.com/mcp?key=${TOOL_KEY:-dev}",
                                      "headers": {"X-Api": "${MISSING_KEY}"}}
                    }},
                    "/Users/ana/Matters/Jones": {"mcpServers": {}},
                    "/Users/ana/Old": {"mcpServers": {}}
                }
            }"#,
        )
        .program("/opt/notes")
        .program("/opt/fs-server")
        .file(
            "/Users/ana/Matters/Jones/.mcp.json",
            r#"{"mcpServers": {"fs": {"command": "/opt/fs-server",
                                     "args": ["${HOME_ROOT:-/tmp}"]}}}"#,
        );
    let pairs = [("NOTES_TOKEN", "SECRET-notes-1")];
    let found = run(Os::Mac, "/Users/ana", &pairs, &fs, &[]);
    assert_eq!(names(&found), ["case-tool", "fs", "notes"]);

    let notes = by_name(&found, "notes");
    assert_eq!(notes.sources, [Source::ClaudeCodeUser]);
    assert_eq!(
        notes.kind,
        local("/opt/notes", &[], &[("TOKEN", "SECRET-notes-1")])
    );
    assert_eq!(notes.problems.len(), 0);

    let case_tool = by_name(&found, "case-tool");
    assert_eq!(
        case_tool.sources,
        [Source::ClaudeCodeProject {
            folder: PathBuf::from("/Users/ana/Matters/Smith")
        }]
    );
    // Project-scope text is kept as written: no expansion, so no unresolved-variable problem.
    assert_eq!(
        case_tool.kind,
        remote(
            "https://tool.example.com/mcp?key=${TOOL_KEY:-dev}",
            &[("X-Api", "${MISSING_KEY}")]
        )
    );
    assert!(case_tool.problems.is_empty(), "{:?}", case_tool.problems);

    let fs_server = by_name(&found, "fs");
    assert_eq!(
        fs_server.sources,
        [Source::ClaudeCodeProject {
            folder: PathBuf::from("/Users/ana/Matters/Jones")
        }]
    );
    // The repository's .mcp.json is kept verbatim and is not approved here (no approval keys).
    assert_eq!(
        fs_server.kind,
        local("/opt/fs-server", &["${HOME_ROOT:-/tmp}"], &[])
    );
    assert_eq!(fs_server.problems, [Problem::NotApproved]);
}

#[test]
fn claude_code_entries_with_unsupported_or_unknown_transports() {
    let fs = FakeFs::default()
        .file(
            "/Users/ana/.claude.json",
            r#"{"mcpServers": {
                "old": {"type": "sse", "url": "https://sse.example.com/sse"},
                "ws": {"type": "websocket", "command": "/bin/ws"},
                "no-target": {"type": "http"},
                "bad-type": {"type": 3, "command": "/bin/x"},
                "not-object": 5
            }}"#,
        )
        .program("/bin/ws");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(names(&found), ["old", "ws"]);
    let old = by_name(&found, "old");
    assert_eq!(old.kind, remote("https://sse.example.com/sse", &[]));
    assert_eq!(
        old.problems,
        [Problem::UnsupportedTransport {
            kind: "sse".to_owned()
        }]
    );
    let ws = by_name(&found, "ws");
    assert_eq!(ws.kind, local("/bin/ws", &[], &[]));
    assert_eq!(
        ws.problems,
        [Problem::UnsupportedTransport {
            kind: "websocket".to_owned()
        }]
    );
}

#[test]
fn codex_local_and_streamable_http_with_env_headers() {
    let fs = FakeFs::default()
        .file(
            "/Users/ana/codex-home/config.toml",
            r#"
[mcp_servers.docs]
command = "/opt/docs"
args = ["--serve"]
env = { DOCS_TOKEN = "SECRET-toml-1" }

[mcp_servers.search]
url = "https://search.example.com/mcp"
bearer_token_env_var = "SEARCH_TOKEN"
http_headers = { "X-Team" = "legal" }
env_http_headers = { "X-Key" = "SEARCH_KEY", "X-Other" = "UNSET_VAR" }

[mcp_servers.off]
command = "/opt/off"
enabled = false

[mcp_servers.bad]
args = ["no command or url"]
"#,
        )
        // $CODEX_HOME wins over ~/.codex.
        .file(
            "/Users/ana/.codex/config.toml",
            r#"[mcp_servers.ignored]
command = "/opt/docs"
"#,
        )
        .program("/opt/docs");
    let pairs = [
        ("CODEX_HOME", "/Users/ana/codex-home"),
        ("SEARCH_TOKEN", "SECRET-token-2"),
        ("SEARCH_KEY", "SECRET-key-3"),
    ];
    let found = run(Os::Mac, "/Users/ana", &pairs, &fs, &[]);
    assert_eq!(names(&found), ["docs", "search"]);

    let docs = by_name(&found, "docs");
    assert_eq!(docs.sources, [Source::Codex]);
    assert_eq!(
        docs.kind,
        local(
            "/opt/docs",
            &["--serve"],
            &[("DOCS_TOKEN", "SECRET-toml-1")]
        )
    );
    assert_eq!(docs.problems.len(), 0);

    let search = by_name(&found, "search");
    assert_eq!(
        search.kind,
        remote(
            "https://search.example.com/mcp",
            &[
                ("Authorization", "Bearer SECRET-token-2"),
                ("X-Key", "SECRET-key-3"),
                ("X-Other", "${UNSET_VAR}"),
                ("X-Team", "legal"),
            ]
        )
    );
    assert_eq!(
        search.problems,
        [Problem::UnresolvedVariable {
            name: "UNSET_VAR".to_owned()
        }]
    );
}

#[test]
fn codex_default_home_and_missing_bearer_variable() {
    let fs = FakeFs::default().file(
        "/Users/ana/.codex/config.toml",
        r#"[mcp_servers.x]
url = "https://x.example.com/mcp"
bearer_token_env_var = "X_TOKEN"
"#,
    );
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    let x = by_name(&found, "x");
    assert_eq!(
        x.kind,
        remote(
            "https://x.example.com/mcp",
            &[("Authorization", "Bearer ${X_TOKEN}")]
        )
    );
    assert_eq!(
        x.problems,
        [Problem::UnresolvedVariable {
            name: "X_TOKEN".to_owned()
        }]
    );
}

#[test]
fn windows_codex_resolves_npx_from_appdata() {
    let fs = FakeFs::default()
        .file(
            "C:\\Users\\ana\\.codex\\config.toml",
            "[mcp_servers.ddg]\ncommand = \"npx\"\n",
        )
        .program("C:\\Users\\ana\\AppData\\Roaming\\npm\\npx.cmd");
    let pairs = [("APPDATA", "C:\\Users\\ana\\AppData\\Roaming")];
    let found = run(Os::Windows, "C:\\Users\\ana", &pairs, &fs, &[]);
    assert_eq!(
        by_name(&found, "ddg").kind,
        local("C:\\Users\\ana\\AppData\\Roaming\\npm\\npx.cmd", &[], &[])
    );
}

#[test]
fn codex_env_vars_are_forwarded_with_their_current_values() {
    let fs = FakeFs::default()
        .file(
            "/Users/ana/.codex/config.toml",
            r#"[mcp_servers.gh]
command = "/opt/gh"
env_vars = [
    "GH_TOKEN",
    "UNSET_ONE",
    "OWN",
    { name = "LOCAL_ONE", source = "local" },
    { name = "REMOTE_ONE", source = "remote" },
]
env = { OWN = "1" }
"#,
        )
        .program("/opt/gh");
    let pairs = [
        ("GH_TOKEN", "hidden-forwarded-four"),
        ("LOCAL_ONE", "hidden-forwarded-five"),
        ("OWN", "from-the-environment"),
    ];
    let found = run(Os::Mac, "/Users/ana", &pairs, &fs, &[]);
    let gh = by_name(&found, "gh");
    assert_eq!(
        gh.kind,
        local(
            "/opt/gh",
            &[],
            &[
                ("GH_TOKEN", "hidden-forwarded-four"),
                ("LOCAL_ONE", "hidden-forwarded-five"),
                ("OWN", "1"),
                ("UNSET_ONE", "${UNSET_ONE}"),
            ]
        )
    );
    // The table's OWN wins over the forwarded one; the remote entry cannot be read here.
    assert_eq!(
        gh.problems,
        [
            Problem::UnresolvedVariable {
                name: "UNSET_ONE".to_owned()
            },
            Problem::Unreadable,
        ]
    );
    let printed = format!("{found:?}");
    assert!(!printed.contains("hidden-forwarded"), "{printed}");
    assert!(!printed.contains("from-the-environment"), "{printed}");
    assert!(printed.contains("GH_TOKEN"), "{printed}");
}
