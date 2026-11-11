//! The size caps: `~/.claude.json` up to 64 MiB, the other configs up to 16 MiB, one byte more is
//! refused, and the real filesystem refuses a file over its cap without reading it whole.

use super::*;

/// A Claude Code config of exactly `total` bytes: a valid server, and padding in a string.
fn padded_config(total: usize) -> String {
    let head = r#"{"mcpServers": {"ok": {"command": "/bin/x"}}, "pad": ""#;
    let tail = r#""}"#;
    let padding = total - head.len() - tail.len();
    format!("{head}{}{tail}", "x".repeat(padding))
}

fn bytes(limit: u64) -> usize {
    usize::try_from(limit).expect("the cap fits in usize")
}

#[test]
fn a_claude_json_one_byte_over_its_cap_is_skipped() {
    let fs = FakeFs::default()
        .file(
            "/Users/ana/.claude.json",
            &padded_config(bytes(MAX_CLAUDE_JSON_BYTES) + 1),
        )
        .program("/bin/x");
    assert_eq!(run(Os::Mac, "/Users/ana", &[], &fs, &[]).len(), 0);
}

#[test]
fn a_claude_json_of_exactly_its_cap_is_read() {
    let fs = FakeFs::default()
        .file(
            "/Users/ana/.claude.json",
            &padded_config(bytes(MAX_CLAUDE_JSON_BYTES)),
        )
        .program("/bin/x");
    assert_eq!(names(&run(Os::Mac, "/Users/ana", &[], &fs, &[])), ["ok"]);
}

#[test]
fn a_project_mcp_json_one_byte_over_the_config_cap_is_skipped() {
    let config =
        r#"{"mcpServers": {}, "projects": {"/Users/ana/Matters/Smith": {"mcpServers": {}}}}"#;
    let mcp = padded_config(bytes(MAX_CONFIG_BYTES) + 1);
    let fs = FakeFs::default()
        .file("/Users/ana/.claude.json", config)
        .file("/Users/ana/Matters/Smith/.mcp.json", &mcp)
        .program("/bin/x");
    assert_eq!(run(Os::Mac, "/Users/ana", &[], &fs, &[]).len(), 0);
}

#[test]
fn a_codex_config_over_its_cap_is_skipped() {
    let mut text = String::from("[mcp_servers.ok]\ncommand = \"/bin/x\"\n# ");
    text.push_str(&"x".repeat(bytes(MAX_CONFIG_BYTES)));
    let fs = FakeFs::default()
        .file("/Users/ana/.codex/config.toml", &text)
        .program("/bin/x");
    assert_eq!(run(Os::Mac, "/Users/ana", &[], &fs, &[]).len(), 0);
}

#[test]
fn the_real_filesystem_refuses_a_file_over_the_cap_without_reading_it_whole() {
    // Cargo.lock is larger than 100 bytes: a cap of 100 refuses it, and a cap of 16 MiB reads it.
    let lock = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let fs = RealFs;
    assert!(fs.read_to_string(&lock, 100).is_none());
    assert!(fs.read_to_string(&lock, MAX_CONFIG_BYTES).is_some());
}
