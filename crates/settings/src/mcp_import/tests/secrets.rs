//! Secrets and what Debug prints: a token on a broken line never echoes, and a candidate's
//! Debug shows names, counts, a URL's scheme and host, a command's file name and no project folder.

use super::*;

#[test]
fn a_broken_config_echoes_nothing_about_its_text() {
    // A token on the line where the JSON and TOML break: no candidate, no message, and
    // nothing in Debug.
    let json =
        r#"{"mcpServers": {"tok": {"command": "/bin/x", "env": {"API_KEY": "hunter-two-probe"}}"#;
    let toml = "[mcp_servers.tok]\ncommand = \"/bin/x\"\nenv = { API_KEY = \"hunter-two-probe\" \n";
    for (path, text) in [
        ("/Users/ana/.claude.json", json),
        ("/Users/ana/.codex/config.toml", toml),
    ] {
        let fs = FakeFs::default().file(path, text).program("/bin/x");
        let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
        assert_eq!(found.len(), 0, "{path}");
        assert!(!format!("{found:?}").contains("hunter-two-probe"));
    }
}

#[test]
fn debug_hides_url_credentials_query_and_arguments() {
    let fs = FakeFs::default().file(
        "/Users/ana/.claude.json",
        r#"{"mcpServers": {
            "remote": {"url": "https://user:hunter-three-probe@r.example.com/mcp?key=hunter-four-probe#frag"},
            "local": {"command": "/bin/x", "args": ["--token", "hunter-five-probe"]}
        }}"#,
    ).program("/bin/x");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    let printed = format!("{found:?}");
    for probe in [
        "hunter-three-probe",
        "hunter-four-probe",
        "hunter-five-probe",
        "user:",
    ] {
        assert!(!printed.contains(probe), "{printed}");
    }
    assert!(printed.contains("\"https://r.example.com\""), "{printed}");
    assert!(!printed.contains("/mcp"), "{printed}");
    assert!(printed.contains("2 not shown"), "{printed}");
}

#[test]
fn debug_names_no_project_folder_and_no_command_directory() {
    // A project folder can be a matter's name; a command's folders can be the lawyer's own.
    let fs = FakeFs::default()
        .file(
            "/Users/ana/.claude.json",
            r#"{"projects": {"/Users/ana/Matters/Smith-Client-Name": {
                "hasTrustDialogAccepted": true,
                "enabledMcpjsonServers": ["tool"],
                "mcpServers": {"tool": {"command": "/Users/ana/secret-tools/bin/mytool"}}
            }}}"#,
        )
        .program("/Users/ana/secret-tools/bin/mytool");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(names(&found), ["tool"]);
    let printed = format!("{found:?}");
    assert!(printed.contains("mytool"), "{printed}");
    assert!(printed.contains("ClaudeCodeProject"), "{printed}");
    for hidden in ["Smith-Client-Name", "Matters", "secret-tools", "/Users/ana"] {
        assert!(!printed.contains(hidden), "{printed}");
    }
}

#[test]
fn import_secret_debug_is_redacted_and_has_no_display() {
    let value = ImportSecret::new("hunter-six-probe".to_owned());
    assert_eq!(format!("{value:?}"), "<redacted>");
    assert_eq!(value.expose(), "hunter-six-probe");
}
