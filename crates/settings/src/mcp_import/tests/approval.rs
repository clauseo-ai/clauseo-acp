//! A project's `.mcp.json` is repository text: its servers need Claude Code's approval, and its
//! text is never expanded. Project folders must be local absolute paths.

use super::*;

/// A project with four `.mcp.json` servers (a, b, c, d), with the given approval keys.
fn project_with(keys: &str) -> FakeFs {
    let config = format!(
        r#"{{"mcpServers": {{}}, "projects": {{"/Users/ana/Matters/Jones": {{{keys}}}}}}}"#
    );
    let mcp = r#"{"mcpServers": {
        "a": {"command": "/bin/x"}, "b": {"command": "/bin/x"},
        "c": {"command": "/bin/x"}, "d": {"command": "/bin/x"}
    }}"#;
    FakeFs::default()
        .file("/Users/ana/.claude.json", &config)
        .file("/Users/ana/Matters/Jones/.mcp.json", mcp)
        .program("/bin/x")
}

/// The names of the candidates that carry no `NotApproved`, and those that do.
fn approved_and_pending(found: &[ImportCandidate]) -> (Vec<&str>, Vec<&str>) {
    let mut approved = Vec::new();
    let mut pending = Vec::new();
    for candidate in found {
        if candidate.problems.contains(&Problem::NotApproved) {
            pending.push(candidate.name.as_str());
        } else {
            approved.push(candidate.name.as_str());
        }
    }
    (approved, pending)
}

#[test]
fn a_listed_server_is_approved_and_a_disabled_one_is_left_out() {
    let fs = project_with(
        r#""hasTrustDialogAccepted": true, "enabledMcpjsonServers": ["a"], "disabledMcpjsonServers": ["c"]"#,
    );
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(names(&found), ["a", "b", "d"]);
    assert_eq!(approved_and_pending(&found), (vec!["a"], vec!["b", "d"]));
}

#[test]
fn enable_all_approves_every_server_of_a_trusted_project() {
    let fs = project_with(r#""hasTrustDialogAccepted": true, "enableAllProjectMcpServers": true"#);
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(
        approved_and_pending(&found),
        (vec!["a", "b", "c", "d"], vec![])
    );
}

#[test]
fn an_untrusted_project_approves_nothing() {
    let fs = project_with(
        r#""hasTrustDialogAccepted": false, "enableAllProjectMcpServers": true, "enabledMcpjsonServers": ["a"]"#,
    );
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(
        approved_and_pending(&found),
        (vec![], vec!["a", "b", "c", "d"])
    );
}

#[test]
fn with_no_approval_keys_every_server_is_pending() {
    let fs = project_with("");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(names(&found), ["a", "b", "c", "d"]);
    assert_eq!(
        approved_and_pending(&found),
        (vec![], vec!["a", "b", "c", "d"])
    );
}

#[test]
fn a_repository_mcp_json_is_never_expanded() {
    let config = r#"{"mcpServers": {}, "projects": {"/Users/ana/Matters/Jones": {
        "hasTrustDialogAccepted": true, "enableAllProjectMcpServers": true,
        "mcpServers": {"own": {"url": "https://own.example.com/${AWS_SECRET_ACCESS_KEY}"}}
    }}}"#;
    let mcp = r#"{"mcpServers": {"repo": {"url": "https://repo.example.com/mcp",
        "headers": {"X": "${AWS_SECRET_ACCESS_KEY}"}}}}"#;
    let fs = FakeFs::default()
        .file("/Users/ana/.claude.json", config)
        .file("/Users/ana/Matters/Jones/.mcp.json", mcp);
    let pairs = [("AWS_SECRET_ACCESS_KEY", "hunter-seven-probe")];
    let found = run(Os::Mac, "/Users/ana", &pairs, &fs, &[]);
    assert_eq!(names(&found), ["own", "repo"]);
    assert_eq!(
        by_name(&found, "repo").kind,
        remote(
            "https://repo.example.com/mcp",
            &[("X", "${AWS_SECRET_ACCESS_KEY}")]
        )
    );
    assert_eq!(by_name(&found, "repo").problems.len(), 0);
    assert_eq!(
        by_name(&found, "own").kind,
        remote("https://own.example.com/${AWS_SECRET_ACCESS_KEY}", &[])
    );
    assert!(!format!("{found:?}").contains("hunter-seven-probe"));
}

#[test]
fn relative_and_network_project_folders_are_skipped() {
    let config = r#"{"mcpServers": {}, "projects": {
        "relative/Matter": {"mcpServers": {"rel": {"command": "/bin/x"}}},
        "//server/share/Matter": {"mcpServers": {"net": {"command": "/bin/x"}}}
    }}"#;
    let fs = FakeFs::default()
        .file("/Users/ana/.claude.json", config)
        .program("/bin/x");
    assert_eq!(run(Os::Mac, "/Users/ana", &[], &fs, &[]).len(), 0);
}
