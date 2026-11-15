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

/// The total cap is shared one entry from each app in turn: a full Claude Desktop and a full
/// Claude Code still leave Codex its servers.
#[test]
fn the_total_cap_never_starves_a_later_app() {
    let servers = |prefix: &str, count: usize| {
        let list: Vec<String> = (0..count)
            .map(|n| format!(r#""{prefix}{n:04}": {{"command": "/bin/x"}}"#))
            .collect();
        format!("{{{}}}", list.join(","))
    };
    let desktop = format!(
        r#"{{"mcpServers": {}}}"#,
        servers("d", MAX_ENTRIES_PER_SOURCE)
    );
    let claude = format!(
        r#"{{"mcpServers": {}, "projects": {{"/Users/ana/A": {{"mcpServers": {}}}}}}}"#,
        servers("u", MAX_ENTRIES_PER_SOURCE),
        servers("a", MAX_ENTRIES_PER_SOURCE),
    );
    let codex = (0..10)
        .map(|n| format!("[mcp_servers.c{n:04}]\ncommand = \"/bin/x\"\n"))
        .collect::<Vec<_>>()
        .concat();
    let fs = FakeFs::default()
        // unix-paths-ok: a fake Mac file system's keys, never the real ones
        .file("/Users/ana/Library/Application Support/Claude/claude_desktop_config.json", &desktop)
        // unix-paths-ok: a fake Mac file system's key
        .file("/Users/ana/.claude.json", &claude)
        // unix-paths-ok: a fake Mac file system's key
        .file("/Users/ana/.codex/config.toml", &codex)
        .program("/bin/x");
    let env_vars = |_: &str| None;
    let env = Env {
        os: Os::Mac,
        // unix-paths-ok: the fake Mac home, read only through the fake file system
        home: Path::new("/Users/ana"),
        vars: &env_vars,
        fs: &fs,
    };
    let (found, left_out) = find_candidates_counted(&env, &[]);
    assert_eq!(found.len(), MAX_ENTRIES_TOTAL);
    assert_eq!(
        left_out,
        3 * MAX_ENTRIES_PER_SOURCE + 10 - MAX_ENTRIES_TOTAL
    );
    let codex = found.iter().filter(|c| c.name.starts_with('c')).count();
    assert_eq!(codex, 10, "Codex was starved");
    let desktop = found.iter().filter(|c| c.name.starts_with('d')).count();
    assert!(desktop > 300, "{desktop} from Claude Desktop");
}
