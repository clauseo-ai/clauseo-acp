//! The keychain Ask's child protocol (`ask_child.rs`): stdin lines in, result lines out.

use super::KeychainItem;
use serde_json::json;

use super::ask_child::{
    AskItem, AskResult, Outcome, request_lines, requested, requests, results, serve,
};

fn item(service: &str, account: &str) -> KeychainItem {
    KeychainItem {
        service: service.to_owned(),
        account: account.to_owned(),
    }
}

#[test]
fn the_flag_starts_the_child_only_as_written() {
    assert!(requested("--keychain-ask".as_ref()));
    for other in ["--keychain-ask=1", "--Keychain-Ask", "keychain-ask"] {
        assert!(!requested(other.as_ref()), "{other}");
    }
}

#[test]
fn the_app_writes_one_line_per_item_and_the_child_reads_them_back() {
    let items = [
        item("ai.clauseo.desktop", "claude-acp"),
        item("ai.clauseo.desktop.mcp", "files \"x\"\nnext"),
    ];
    let lines = request_lines(&items);
    assert_eq!(lines.lines().count(), 2);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(lines.lines().next().unwrap()).unwrap(),
        json!({"service": "ai.clauseo.desktop", "account": "claude-acp"})
    );
    let back: Vec<KeychainItem> = requests(lines.as_bytes())
        .into_iter()
        .map(|asked| item(&asked.service, &asked.account))
        .collect();
    assert_eq!(back, items);
}

#[test]
fn the_child_reads_each_item_in_order_and_answers_never_a_value() {
    let input = request_lines(&[
        item("ai.clauseo.desktop", "claude-acp"),
        item("ai.clauseo.desktop", "codex-acp"),
        item("ai.clauseo.desktop.mcp", "files"),
    ]) + "not json\n\n";
    let mut read_order = Vec::new();
    let mut output = Vec::new();
    let code = serve(input.as_bytes(), &mut output, |asked: &AskItem| {
        read_order.push(asked.account.clone());
        match asked.account.as_str() {
            "claude-acp" => Ok(true),
            "codex-acp" => Err(-128),
            _ => Ok(false),
        }
    });
    assert_eq!(code, 0);
    assert_eq!(read_order, ["claude-acp", "codex-acp", "files"]);
    let output = String::from_utf8(output).unwrap();
    let lines: Vec<serde_json::Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            json!({"service": "ai.clauseo.desktop", "account": "claude-acp", "outcome": "read"}),
            json!({"service": "ai.clauseo.desktop", "account": "codex-acp", "outcome": "refused",
                   "code": -128}),
            json!({"service": "ai.clauseo.desktop.mcp", "account": "files", "outcome": "gone"}),
        ]
    );
    // The app reads them back.
    let answers = results(&output);
    assert_eq!(
        answers
            .iter()
            .map(|answer| answer.outcome)
            .collect::<Vec<_>>(),
        [Outcome::Read, Outcome::Refused, Outcome::Gone]
    );
    assert_eq!(answers[1].code, Some(-128));
    assert!(answers[1].is_for(&item("ai.clauseo.desktop", "codex-acp")));
}

#[test]
fn the_app_skips_lines_it_cannot_parse() {
    let answers =
        results("garbage\n{\"service\":\"s\",\"account\":\"a\",\"outcome\":\"read\"}\n{}\n");
    assert_eq!(
        answers,
        [AskResult {
            service: "s".to_owned(),
            account: "a".to_owned(),
            outcome: Outcome::Read,
            code: None,
        }]
    );
}

#[test]
fn a_child_whose_app_has_gone_stops() {
    struct Closed;
    impl std::io::Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let input = request_lines(&[
        item("ai.clauseo.desktop", "a"),
        item("ai.clauseo.desktop", "b"),
    ]);
    let mut reads = 0;
    let code = serve(input.as_bytes(), Closed, |_| {
        reads += 1;
        Ok(true)
    });
    assert_eq!((code, reads), (1, 1));
}

#[test]
fn a_foreign_service_is_never_read_nor_answered() {
    let input = request_lines(&[
        item("com.apple.account.IdentityServices", "token"),
        item("ai.clauseo.desktop", "claude-acp"),
        item("Claude Code-credentials", "ada"),
        item("ai.clauseo.desktop.evil", "x"),
        item("AI.CLAUSEO.DESKTOP", "claude-acp"),
    ]);
    let mut read = Vec::new();
    let mut output = Vec::new();
    serve(input.as_bytes(), &mut output, |asked: &AskItem| {
        read.push(asked.service.clone());
        Ok(true)
    });
    assert_eq!(read, ["ai.clauseo.desktop"]);
    assert_eq!(String::from_utf8(output).unwrap().lines().count(), 1);
}

#[test]
fn each_of_clauseo_s_services_is_read() {
    let items: Vec<KeychainItem> = super::system::KEYCHAIN_SERVICES
        .iter()
        .map(|service| item(service, "a"))
        .collect();
    assert_eq!(requests(request_lines(&items).as_bytes()).len(), 4);
}

#[test]
fn at_most_64_request_lines_are_read() {
    let items: Vec<KeychainItem> = (0..100)
        .map(|n| item("ai.clauseo.desktop", &format!("agent-{n}")))
        .collect();
    let read = requests(request_lines(&items).as_bytes());
    assert_eq!(read.len(), super::ask_child::MAX_REQUESTS);
    assert_eq!(read.last().unwrap().account, "agent-63");
    // A dropped line counts too: 64 foreign lines leave no room for a later one of Clauseo's.
    let mut foreign: Vec<KeychainItem> = (0..64).map(|n| item("other", &n.to_string())).collect();
    foreign.push(item("ai.clauseo.desktop", "late"));
    assert_eq!(requests(request_lines(&foreign).as_bytes()), []);
}

#[test]
fn a_line_too_long_is_dropped_and_the_next_still_read() {
    let long = "x".repeat(super::ask_child::LINE_MAX * 4);
    let input = request_lines(&[
        item("ai.clauseo.desktop", &long),
        item("ai.clauseo.desktop", "claude-acp"),
    ]);
    let read = requests(input.as_bytes());
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].account, "claude-acp");
    // A line just within the bound reads.
    let fits = json!({"service": "ai.clauseo.desktop", "account": ""}).to_string();
    let account = "y".repeat(super::ask_child::LINE_MAX - fits.len());
    let input = request_lines(&[item("ai.clauseo.desktop", &account)]);
    assert_eq!(input.len(), super::ask_child::LINE_MAX + 1);
    assert_eq!(requests(input.as_bytes()).len(), 1);
    // One byte more is dropped; a last line with no newline still reads.
    let input = request_lines(&[item("ai.clauseo.desktop", &format!("{account}y"))]);
    assert_eq!(requests(input.as_bytes()), []);
    let last = json!({"service": "ai.clauseo.desktop", "account": "a"}).to_string();
    assert_eq!(requests(last.as_bytes()).len(), 1);
}
