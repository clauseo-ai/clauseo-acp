//! The skip rules: which problems block a server, and which reason a server with several gets.

use std::path::PathBuf;

use clauseo_protocol::{McpImportSource, McpSkipReason};
use clauseo_settings::mcp_import::{Problem, Source};

use super::{skip_of, source_of};

#[test]
fn a_changed_name_alone_does_not_block() {
    let renamed = [Problem::NameChanged {
        from: "my server".to_owned(),
    }];
    assert_eq!(skip_of(&renamed), None);
    assert_eq!(skip_of(&[]), None);
}

#[test]
fn each_blocking_problem_gives_its_reason() {
    let cases = [
        (Problem::AlreadyAdded, McpSkipReason::AlreadyAdded),
        (Problem::NotApproved, McpSkipReason::NotApproved),
        (
            Problem::UnsupportedTransport {
                kind: "sse".to_owned(),
            },
            McpSkipReason::UnsupportedTransport,
        ),
        (
            Problem::UnresolvedVariable {
                name: "TOKEN".to_owned(),
            },
            McpSkipReason::UnresolvedVariable,
        ),
        (Problem::Unreadable, McpSkipReason::Unreadable),
    ];
    for (problem, reason) in cases {
        assert_eq!(skip_of(&[problem]), Some((reason, None)), "{reason:?}");
    }
    let lost = [Problem::CommandNotFound {
        original: "uvx".to_owned(),
    }];
    assert_eq!(
        skip_of(&lost),
        Some((McpSkipReason::CommandNotFound, Some("uvx".to_owned())))
    );
}

#[test]
fn the_first_reason_in_declaration_order_wins() {
    let problems = [
        Problem::Unreadable,
        Problem::CommandNotFound {
            original: "uvx".to_owned(),
        },
        Problem::AlreadyAdded,
    ];
    assert_eq!(
        skip_of(&problems),
        Some((McpSkipReason::AlreadyAdded, None))
    );
}

#[test]
fn claude_codes_two_scopes_are_one_source() {
    assert_eq!(
        source_of(&Source::ClaudeDesktop),
        McpImportSource::ClaudeDesktop
    );
    assert_eq!(
        source_of(&Source::ClaudeCodeUser),
        McpImportSource::ClaudeCode
    );
    let project = Source::ClaudeCodeProject {
        folder: PathBuf::from("/Users/ana/Matters/Jones"),
    };
    assert_eq!(source_of(&project), McpImportSource::ClaudeCode);
    assert_eq!(source_of(&Source::Codex), McpImportSource::Codex);
}
