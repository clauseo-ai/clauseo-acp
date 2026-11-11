//! Names, merges, problems and what Debug prints.

use super::*;

#[test]
fn names_are_cleaned_to_the_mcp_rules() {
    let long = "x".repeat(70);
    let config = format!(
        r#"{{"mcpServers": {{
            "a__b": {{"command": "/bin/x"}},
            "my server": {{"command": "/bin/x"}},
            "trail_": {{"command": "/bin/x"}},
            "___": {{"command": "/bin/x"}},
            "browser": {{"command": "/bin/x"}},
            "Case-Law": {{"command": "/bin/x"}},
            "héllo": {{"command": "/bin/x"}},
            "{long}": {{"command": "/bin/x"}},
            "ok_name-1": {{"command": "/bin/x"}}
        }}}}"#
    );
    let fs = FakeFs::default()
        .file("/Users/ana/.claude.json", &config)
        .program("/bin/x");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    let expected = [
        "Case-Law-imported",
        "a_b",
        "browser-imported",
        "h-llo",
        "my-server",
        "ok_name-1",
        "server",
        "trail",
        &long[..64],
    ];
    assert_eq!(names(&found), expected);

    for candidate in &found {
        assert!(
            candidate.name.len() <= 64
                && !candidate.name.contains("__")
                && !candidate.name.ends_with('_')
                && candidate
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
            "{}",
            candidate.name
        );
        assert!(
            crate::reserved_mcp_server(&candidate.name).is_none(),
            "{}",
            candidate.name
        );
    }
    assert_eq!(
        by_name(&found, "a_b").problems,
        [Problem::NameChanged {
            from: "a__b".to_owned()
        }]
    );
    assert_eq!(
        by_name(&found, "browser-imported").problems,
        [Problem::NameChanged {
            from: "browser".to_owned()
        }]
    );
    assert_eq!(
        by_name(&found, "Case-Law-imported").problems,
        [Problem::NameChanged {
            from: "Case-Law".to_owned()
        }]
    );
    assert_eq!(by_name(&found, "ok_name-1").problems.len(), 0);
}

#[test]
fn same_config_in_several_sources_is_one_candidate() {
    let fs = FakeFs::default()
        .file(
            DESKTOP_MAC,
            r#"{"mcpServers": {"dup": {"command": "/bin/x", "args": ["a"],
                                       "env": {"A": "1", "B": "2"}}}}"#,
        )
        .file(
            "/Users/ana/.claude.json",
            r#"{"mcpServers": {"dup": {"command": "/bin/x", "args": ["a"],
                                       "env": {"B": "2", "A": "1"}}}}"#,
        )
        .program("/bin/x");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(names(&found), ["dup"]);
    assert_eq!(
        found[0].sources,
        [Source::ClaudeDesktop, Source::ClaudeCodeUser]
    );
    assert_eq!(found[0].problems.len(), 0);
}

#[test]
fn a_different_config_under_the_same_name_gets_a_suffix() {
    let fs = FakeFs::default()
        .file(
            DESKTOP_MAC,
            r#"{"mcpServers": {"dup": {"command": "/bin/x", "args": ["a"]}}}"#,
        )
        .file(
            "/Users/ana/.claude.json",
            r#"{"mcpServers": {"dup": {"command": "/bin/x", "args": ["b"]}}}"#,
        )
        .file(
            "/Users/ana/.codex/config.toml",
            "[mcp_servers.dup]\ncommand = \"/bin/x\"\nargs = [\"c\"]\n",
        )
        .program("/bin/x");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(names(&found), ["dup", "dup-2", "dup-3"]);
    assert_eq!(found[0].sources, [Source::ClaudeDesktop]);
    assert_eq!(found[1].sources, [Source::ClaudeCodeUser]);
    assert_eq!(found[2].sources, [Source::Codex]);
    for renamed in &found[1..] {
        assert_eq!(
            renamed.problems,
            [Problem::NameChanged {
                from: "dup".to_owned()
            }]
        );
    }
}

#[test]
fn suffixes_skip_names_taken_by_clauseo_or_by_other_servers() {
    let fs = FakeFs::default()
        .file(
            "/Users/ana/.claude.json",
            r#"{"mcpServers": {
                "a": {"command": "/bin/x", "args": ["1"]},
                "a-2": {"command": "/bin/x", "args": ["3"]}
            }}"#,
        )
        .file(
            "/Users/ana/.codex/config.toml",
            "[mcp_servers.a]\ncommand = \"/bin/x\"\nargs = [\"2\"]\n",
        )
        .program("/bin/x");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(names(&found), ["a", "a-2", "a-3"]);

    let fs = FakeFs::default()
        .file(
            "/Users/ana/.claude.json",
            r#"{"mcpServers": {"dup": {"command": "/bin/x", "args": ["1"]}}}"#,
        )
        .file(
            "/Users/ana/.codex/config.toml",
            "[mcp_servers.dup]\ncommand = \"/bin/x\"\nargs = [\"2\"]\n",
        )
        .program("/bin/x");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &["dup-2"]);
    assert_eq!(names(&found), ["dup", "dup-3"]);
}

#[test]
fn names_already_in_clauseo_are_marked_added() {
    let fs = FakeFs::default()
        .file(
            "/Users/ana/.claude.json",
            r#"{"mcpServers": {"files": {"command": "/bin/x"}, "web": {"command": "/bin/x"}}}"#,
        )
        .program("/bin/x");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &["files"]);
    assert_eq!(by_name(&found, "files").problems, [Problem::AlreadyAdded]);
    assert_eq!(by_name(&found, "web").problems.len(), 0);

    let found = run(Os::Mac, "/Users/ana", &[], &fs, &["web"]);
    assert_eq!(by_name(&found, "web").problems, [Problem::AlreadyAdded]);
}

#[test]
fn unreadable_parts_are_left_out_and_flagged() {
    let fs = FakeFs::default()
        .file(
            "/Users/ana/.claude.json",
            r#"{"mcpServers": {"odd": {"command": "/bin/x", "args": "not-a-list",
                                       "env": {"GOOD": "1", "BAD": 5}}}}"#,
        )
        .program("/bin/x");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    let odd = by_name(&found, "odd");
    assert_eq!(odd.kind, local("/bin/x", &[], &[("GOOD", "1")]));
    assert_eq!(odd.problems, [Problem::Unreadable]);
}

#[test]
fn unreadable_files_give_no_candidates_and_never_panic() {
    let cases = [
        "{ not json",
        "[1, 2, 3]",
        r#"{"mcpServers": [1]}"#,
        r#"{"projects": 4}"#,
        "",
    ];
    for text in cases {
        let fs = FakeFs::default()
            .file("/Users/ana/.claude.json", text)
            .file(DESKTOP_MAC, text)
            .file("/Users/ana/.codex/config.toml", text);
        assert_eq!(run(Os::Mac, "/Users/ana", &[], &fs, &[]).len(), 0, "{text}");
    }
    let bad_toml = FakeFs::default().file(
        "/Users/ana/.codex/config.toml",
        "[mcp_servers.x\ncommand = ",
    );
    assert_eq!(run(Os::Mac, "/Users/ana", &[], &bad_toml, &[]).len(), 0);
    let not_a_table = FakeFs::default().file("/Users/ana/.codex/config.toml", "mcp_servers = 3\n");
    assert_eq!(run(Os::Mac, "/Users/ana", &[], &not_a_table, &[]).len(), 0);
    assert_eq!(
        run(Os::Mac, "/Users/ana", &[], &FakeFs::default(), &[]).len(),
        0
    );
}

#[test]
fn output_is_sorted_by_name() {
    let fs = FakeFs::default()
        .file(
            "/Users/ana/.claude.json",
            r#"{"mcpServers": {"zeta": {"command": "/bin/x"}, "alpha": {"command": "/bin/x"}, "Mid": {"command": "/bin/x"}}}"#,
        )
        .program("/bin/x");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(names(&found), ["Mid", "alpha", "zeta"]);
}

#[test]
fn debug_prints_names_never_values() {
    let fs = FakeFs::default()
        .file(
            DESKTOP_MAC,
            r#"{"mcpServers": {
                "local": {"command": "/bin/x", "env": {"API_KEY": "SECRET-env-value"}},
                "remote": {"url": "https://r.example.com/mcp?token=SECRET-query",
                           "headers": {"Authorization": "SECRET-header-value"}}
            }}"#,
        )
        .program("/bin/x");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_eq!(found.len(), 2);
    let printed = format!("{found:?}");
    assert!(!printed.contains("SECRET"), "{printed}");
    assert!(printed.contains("API_KEY"), "{printed}");
    assert!(printed.contains("Authorization"), "{printed}");
    assert!(printed.contains("\"https://r.example.com\""), "{printed}");
    assert!(!printed.contains("/mcp"), "{printed}");
    let kind = format!("{:?}", found[1].kind);
    assert!(!kind.contains("SECRET"), "{kind}");
}

/// leaf/mcp.rs's name rule, copied (its `mcp_name_ok` is private): 1 to 64 ASCII letters, digits,
/// `_` or `-`; no `__`; not ending in `_`; not a reserved name in any case.
fn leaf_name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !name.contains("__")
        && !name.ends_with('_')
        && crate::reserved_mcp_server(name).is_none()
}

#[test]
fn every_imported_name_passes_the_leaf_rule() {
    let raws: Vec<String> = [
        "",
        "_",
        "__",
        "___",
        "-",
        "--",
        "-lead",
        "_lead",
        "a--b",
        "a__",
        "a___b",
        "trail-",
        "x/y",
        "a b",
        "a.b",
        "héllo",
        "名前",
        "x\u{0}y",
        "browser",
        "BROWSER",
        "Browser_",
        "case-law",
        "Case-Law_",
        "case-law__x",
        "a-2",
        "a 2",
        "x_-y",
        "%%%",
        "\n",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([
        "x".repeat(63),
        "x".repeat(64),
        "x".repeat(65),
        "x".repeat(200),
        format!("{}_", "y".repeat(64)),
    ])
    .collect();
    let mut servers = serde_json::Map::new();
    for raw in &raws {
        servers.insert(raw.clone(), serde_json::json!({"command": "/bin/x"}));
    }
    let config = serde_json::json!({"mcpServers": servers}).to_string();
    let fs = FakeFs::default()
        .file("/Users/ana/.claude.json", &config)
        .program("/bin/x");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &[]);
    assert_ne!(found.len(), 0);
    let mut seen = std::collections::BTreeSet::new();
    for candidate in &found {
        assert!(leaf_name_ok(&candidate.name), "{:?}", candidate.name);
        assert!(
            seen.insert(candidate.name.clone()),
            "duplicate {:?}",
            candidate.name
        );
    }
}

/// Names compare case-blind (Credential Manager's accounts do): a config named in another case
/// is one base name with a suffix, and a name Clauseo has in any case is already added.
#[test]
fn names_compare_case_blind() {
    let claude = r#"{"mcpServers": {
        "GitHub": {"command": "/bin/x"},
        "github": {"command": "/bin/y"},
        "Search": {"command": "/bin/x"}
    }}"#;
    let fs = FakeFs::default()
        .file("/Users/ana/.claude.json", claude)
        .program("/bin/x")
        .program("/bin/y");
    let found = run(Os::Mac, "/Users/ana", &[], &fs, &["search"]);
    let mut lowered: Vec<String> = found.iter().map(|c| c.name.to_ascii_lowercase()).collect();
    lowered.sort();
    lowered.dedup();
    assert_eq!(
        lowered.len(),
        found.len(),
        "two names differ only in case: {:?}",
        names(&found)
    );
    assert!(names(&found).contains(&"github-2") || names(&found).contains(&"GitHub-2"));
    let search = found.iter().find(|c| c.name == "Search").unwrap();
    assert!(search.problems.contains(&Problem::AlreadyAdded));
}

/// One source reads at most `MAX_ENTRIES_PER_SOURCE` servers; the rest are counted.
#[test]
fn a_source_reads_at_most_its_cap_and_counts_the_rest() {
    let servers: Vec<String> = (0..MAX_ENTRIES_PER_SOURCE + 7)
        .map(|n| format!(r#""s{n:04}": {{"command": "/bin/x"}}"#))
        .collect();
    let claude = format!(r#"{{"mcpServers": {{{}}}}}"#, servers.join(","));
    let fs = FakeFs::default()
        .file("/Users/ana/.claude.json", &claude)
        .program("/bin/x");
    let env_vars = |_: &str| None;
    let env = Env {
        os: Os::Mac,
        home: Path::new("/Users/ana"),
        vars: &env_vars,
        fs: &fs,
    };
    let (found, left_out) = find_candidates_counted(&env, &[]);
    assert_eq!(found.len(), MAX_ENTRIES_PER_SOURCE);
    assert_eq!(left_out, 7);
}
