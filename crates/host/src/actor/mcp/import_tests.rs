//! The skip rules: which problems block a server, which reason a server with several gets, and
//! that a blocked one is never made ready to import.

use std::path::PathBuf;

use clauseo_protocol::{McpImportSource, McpSkipReason};
use clauseo_settings::mcp_import::{CandidateKind, ImportCandidate, Problem, Source};

use super::{pick, program_name, skip_of, source_of};

fn candidate(name: &str, source: Source, problems: Vec<Problem>) -> ImportCandidate {
    ImportCandidate {
        name: name.to_owned(),
        sources: vec![source],
        kind: CandidateKind::Local {
            command: PathBuf::from("/opt/homebrew/bin/npx"),
            args: Vec::new(),
            env: Vec::new(),
        },
        problems,
    }
}

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

#[test]
fn an_unapproved_or_problem_candidate_is_never_made_ready() {
    let project = Source::ClaudeCodeProject {
        folder: PathBuf::from("/Users/ana/Matters/Jones"),
    };
    let candidates = vec![
        candidate("repo-tool", project, vec![Problem::NotApproved]),
        // Held in another case: D does not mark it, the host does.
        candidate("added", Source::ClaudeCodeUser, Vec::new()),
        candidate("broken", Source::ClaudeCodeUser, vec![Problem::Unreadable]),
        candidate(
            "unset",
            Source::ClaudeCodeUser,
            vec![Problem::UnresolvedVariable {
                name: "TOKEN".to_owned(),
            }],
        ),
        candidate("fine", Source::ClaudeCodeUser, Vec::new()),
        candidate("other-app", Source::Codex, Vec::new()),
    ];
    let (ready, skipped) = pick(
        candidates,
        McpImportSource::ClaudeCode,
        &["ADDED".to_owned()],
    );
    let ready: Vec<&str> = ready.iter().map(|one| one.name.as_str()).collect();
    assert_eq!(ready, ["fine"]);
    assert!(ready.iter().all(|name| *name != "other-app"));
    let skipped: Vec<(&str, McpSkipReason)> = skipped
        .iter()
        .map(|one| (one.name.as_str(), one.reason))
        .collect();
    assert_eq!(
        skipped,
        [
            ("repo-tool", McpSkipReason::NotApproved),
            ("added", McpSkipReason::AlreadyAdded),
            ("broken", McpSkipReason::Unreadable),
            ("unset", McpSkipReason::UnresolvedVariable),
        ]
    );
}

#[test]
fn a_ready_server_arrives_turned_off() {
    let (ready, _) = pick(
        vec![candidate("fine", Source::Codex, Vec::new())],
        McpImportSource::Codex,
        &[],
    );
    assert!(ready.iter().all(|one| !one.server.enabled));
    assert_eq!(ready.len(), 1);
}

#[test]
fn a_missing_program_is_named_by_its_file_name_only() {
    assert_eq!(program_name("/Users/ana/tools/uvx"), "uvx");
    assert_eq!(program_name("C:\\Users\\ana\\uvx.cmd"), "uvx.cmd");
    assert_eq!(program_name("uvx"), "uvx");
}
