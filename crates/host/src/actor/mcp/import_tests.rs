//! The skip rules: which problems block a server, which reason a server with several gets, and
//! that a blocked one is never made ready to import.

use std::path::PathBuf;

use clauseo_protocol::{McpImportSource, McpSkipReason};
use clauseo_settings::mcp_import::{CandidateKind, ImportCandidate, Problem, Source};

use super::fit::{fits_alone, fitting};
use super::{pick, program_name, skip_of, source_of};
use crate::config::mcp::McpLaunchAsk;
use crate::config::mcp::shown::MAX_ASK_CHARS;
use crate::testing::abs;

fn candidate(name: &str, source: Source, problems: Vec<Problem>) -> ImportCandidate {
    ImportCandidate {
        name: name.to_owned(),
        sources: vec![source],
        kind: CandidateKind::Local {
            command: PathBuf::from(abs("/opt/homebrew/bin/npx")),
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

fn ask(server: &str, args: &[String]) -> McpLaunchAsk {
    McpLaunchAsk {
        server: server.to_owned(),
        program: std::iter::once("npx".to_owned())
            .chain(args.iter().cloned())
            .collect(),
        url: None,
        names: Vec::new(),
    }
}

/// The import's cap counts the list as shown: `a (npx ` and `)` are 8 characters, and each
/// newline shows as `\n`, so 996 newlines show as exactly 2,000 characters.
#[test]
fn an_imports_cap_counts_escapes_as_shown_at_the_boundary() {
    let at_cap = "\n".repeat((MAX_ASK_CHARS - 8) / 2);
    assert!(fits_alone(&ask("a", std::slice::from_ref(&at_cap))));
    let over = format!("{at_cap}\n");
    assert!(!fits_alone(&ask("a", &[over])));
    // Quotes and backslashes: `it's` puts it in double quotes (2 more), each `\` shows as two.
    let quoted = format!("it's{}", "\\".repeat((MAX_ASK_CHARS - 8 - 6) / 2));
    assert!(fits_alone(&ask("a", std::slice::from_ref(&quoted))));
    assert!(!fits_alone(&ask("a", &[format!("{quoted}\\")])));
}

/// Whole servers, from the first, while the list as shown fits; a second server that would pass
/// the cap by one character is left out whole, and so is every one after it, even a short one.
#[test]
fn an_import_keeps_a_whole_prefix_that_fits() {
    // `a (npx …)`, `, ` and `b (npx)`: 8 + n + 2 + 7 characters.
    let first = "𝔄".repeat(MAX_ASK_CHARS - 17);
    let asks = [ask("a", std::slice::from_ref(&first)), ask("b", &[])];
    assert_eq!(fitting(&asks), 2);
    let longer = format!("{first}𝔄");
    let asks = [ask("a", &[longer]), ask("b", &[]), ask("c", &[])];
    assert_eq!(fitting(&asks), 1);
    assert_eq!(fitting(&[]), 0);
}

/// A store whose every `set` from the second on fails; reads and deletes go to `inner`.
struct FailFromSecond {
    inner: std::sync::Arc<clauseo_connectors::MemoryStore>,
    sets: std::sync::atomic::AtomicUsize,
}

impl clauseo_connectors::SecretStore for FailFromSecond {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, clauseo_connectors::StoreError> {
        self.inner.get(account)
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<(), clauseo_connectors::StoreError> {
        if self.sets.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 1 {
            return Err(clauseo_connectors::StoreError::Platform(
                "refused".to_owned(),
            ));
        }
        self.inner.set(account, secret)
    }

    fn delete(&self, account: &str) -> Result<(), clauseo_connectors::StoreError> {
        self.inner.delete(account)
    }
}

fn ready(name: &str, accounts: &[&str]) -> super::Ready {
    super::Ready {
        name: name.to_owned(),
        server: clauseo_settings::McpServerLaunch::local(
            "npx".to_owned(),
            Vec::new(),
            std::collections::BTreeMap::new(),
            false,
        ),
        steps: accounts
            .iter()
            .map(|account| super::Step::Set {
                account: (*account).to_owned(),
                value: zeroize::Zeroizing::new("new".to_owned()),
            })
            .collect(),
    }
}

/// The second server's write fails, and so does the put-back's restore of the first server's
/// earlier item: the import still answers the store's error, and the put-back goes on to delete
/// the item the first server added, so only the one item the keychain refused twice is left.
#[test]
fn a_rollback_whose_own_writes_fail_still_answers_and_puts_back_the_rest() {
    let inner = std::sync::Arc::new(clauseo_connectors::MemoryStore::new());
    inner.put("a/env/OLD", b"earlier");
    let store = FailFromSecond {
        inner: std::sync::Arc::clone(&inner),
        sets: std::sync::atomic::AtomicUsize::new(0),
    };
    // `a/env/NEW` lands (the first set); `a/env/OLD` is refused, so `a` fails on its own.
    let result = super::write(Some(&store), vec![ready("a", &["a/env/NEW", "a/env/OLD"])]);
    assert!(matches!(result, Err(Some(_))));
    assert_eq!(
        inner.peek("a/env/NEW"),
        None,
        "the added item was not deleted"
    );
    assert_eq!(inner.peek("a/env/OLD").as_deref(), Some(&b"earlier"[..]));
    // Across servers: `b` lands, `c` is refused, and `b`'s restore is refused too.
    let inner = std::sync::Arc::new(clauseo_connectors::MemoryStore::new());
    inner.put("b/env/B", b"earlier");
    let store = FailFromSecond {
        inner: std::sync::Arc::clone(&inner),
        sets: std::sync::atomic::AtomicUsize::new(0),
    };
    let result = super::write(
        Some(&store),
        vec![ready("b", &["b/env/B"]), ready("c", &["c/env/C"])],
    );
    assert!(matches!(result, Err(Some(_))));
    assert_eq!(inner.peek("c/env/C"), None);
    // The keychain refused the restore: the item keeps the import's value, and the count is logged.
    assert_eq!(inner.peek("b/env/B").as_deref(), Some(&b"new"[..]));
}
