//! The lawyer's side of "Always for this matter" (`actor/kept.rs`): `KeptPermissions` lists a
//! matter's kept rules in plain kinds, `RemoveKeptPermission` takes one out of the file and out
//! of the running agent (even when the file never held it, and from a launch that read the file
//! before the removal), and an "Always" that kept nothing logs a notice saying it was allowed
//! this once only.

use std::path::Path;

use clauseo_protocol::{
    CommandOutcome, DomainEvent, HostCommand, HostQuery, KeptKind, KeptPermission, MatterId,
    NotKeptReason, Notice, NoticeKind, NoticeLevel, QueryResult, StopReason, StreamId,
};

use crate::permissions::answer_from;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

const CLAUDE: &str = "claude-acp";

/// Writes `rules` as `matter`'s kept rules, as the host stores them.
fn keep(dir: &Dir, matter: MatterId, rules: &[&str]) {
    let file = dir
        .data()
        .join("agent-state")
        .join(matter.to_string())
        .join("claude-allow.json");
    let text = serde_json::json!({"version": 1, "allow": rules}).to_string();
    crate::write::write(&file, text.as_bytes());
}

/// The rule form of `path`, an absolute path: `//x/y` for `/x/y`, `//c/x/y` for `C:\x\y`.
fn rule_form(path: &Path) -> String {
    let text = path.to_str().unwrap().replace('\\', "/");
    if let Some(rest) = text.strip_prefix('/') {
        return format!("//{rest}");
    }
    let (drive, rest) = text.split_at(1);
    format!("//{}{}", drive.to_lowercase(), &rest[1..])
}

fn kept(host: &Running, matter: MatterId) -> Vec<KeptPermission> {
    match host.query(HostQuery::KeptPermissions { matter }) {
        Ok(QueryResult::KeptPermissions(kept)) => kept,
        other => panic!("KeptPermissions answered {other:?}"),
    }
}

fn notices(events: &crate::observe::Events) -> Vec<Notice> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => Some(notice.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_kept_rules_are_listed_in_plain_kinds() {
    let dir = Dir::new("kept_list", &[(CLAUDE, Scenario::Local("claude-always"))]);
    let host = dir.start(0);
    let (matter, _thread, folder) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    assert_eq!(kept(&host, matter), Vec::new(), "none kept yet");
    let edit = format!("Edit({}/work/out.txt)", rule_form(&folder));
    let outside = std::env::temp_dir().join("elsewhere.txt");
    let read = format!("Read({})", rule_form(&outside));
    let rules = [
        "Bash(npm test)",
        "WebFetch(domain:example.com)",
        "mcp__court__find_case",
        edit.as_str(),
        read.as_str(),
    ];
    keep(&dir, matter, &rules);
    let listed = kept(&host, matter);
    let shown: Vec<(&str, KeptKind, &str, Option<&str>)> = listed
        .iter()
        .map(|k| {
            (
                k.rule.as_str(),
                k.kind,
                k.subject.as_str(),
                k.server.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        shown[..4],
        [
            ("Bash(npm test)", KeptKind::Run, "npm test", None),
            (
                "WebFetch(domain:example.com)",
                KeptKind::Site,
                "example.com",
                None
            ),
            (
                "mcp__court__find_case",
                KeptKind::Tool,
                "find_case",
                Some("court")
            ),
            (edit.as_str(), KeptKind::Edit, "work/out.txt", None),
        ]
    );
    // A file outside the matter is shown whole.
    let last = &listed[4];
    assert_eq!(
        (last.rule.as_str(), last.kind),
        (read.as_str(), KeptKind::Read)
    );
    assert!(
        last.subject.ends_with("elsewhere.txt") && last.subject.len() > "elsewhere.txt".len(),
        "{last:?}"
    );
    host.shutdown();
}

#[test]
fn a_removed_rule_leaves_the_file_and_the_running_agent() {
    let dir = Dir::new(
        "kept_remove",
        &[(CLAUDE, Scenario::Local("claude-always-remove"))],
    );
    let host = dir.start(0);
    let (matter, thread, _folder) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    keep(&dir, matter, &["Bash(npm test)", "Bash(npm run lint)"]);
    let mut events = host.events(StreamId::Thread(thread));
    // The first npm test is the host's to allow, from the kept rule.
    let (turn, _) = host.prompt(thread, "Run the tests.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let remove = |rule: &str| {
        host.ok(HostCommand::RemoveKeptPermission {
            matter,
            rule: rule.to_owned(),
        })
    };
    assert_eq!(remove("Bash(npm test)"), CommandOutcome::Done);
    // Removing a rule that isn't there does nothing.
    assert_eq!(remove("Bash(npm test)"), CommandOutcome::Done);
    let left: Vec<String> = kept(&host, matter).into_iter().map(|k| k.rule).collect();
    assert_eq!(left, ["Bash(npm run lint)"]);
    // The same session asks the lawyer again; the scenario expects the lawyer's reject.
    let (turn, _) = host.prompt(thread, "Run them again.");
    answer_from(&host, &mut events, thread, 1, &["reject"]);
    events.until_turn_ended(turn);
    events.assert_no_mismatch();
    host.shutdown();
}

/// A launch that read the file before the lawyer's removal reached it never applies the rule:
/// its agent is built without the rules removed since (`actor/kept.rs` `Ledger`). The rule is
/// written back after the removal, as such a launch read it.
#[test]
fn a_rule_removed_while_a_launch_read_it_is_not_applied() {
    let dir = Dir::new(
        "kept_launch_race",
        &[(CLAUDE, Scenario::Local("claude-always-remove"))],
    );
    let host = dir.start(0);
    let (matter, thread, _folder) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    keep(&dir, matter, &["Bash(npm test)"]);
    let remove = HostCommand::RemoveKeptPermission {
        matter,
        rule: "Bash(npm test)".to_owned(),
    };
    assert_eq!(host.ok(remove), CommandOutcome::Done);
    keep(&dir, matter, &["Bash(npm test)"]);
    // The lawyer is asked both times: allow once, then reject.
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Run the tests.");
    answer_from(&host, &mut events, thread, 0, &["allow-once"]);
    events.until_turn_ended(turn);
    let (turn, _) = host.prompt(thread, "Run them again.");
    answer_from(&host, &mut events, thread, 1, &["reject"]);
    events.until_turn_ended(turn);
    events.assert_no_mismatch();
    host.shutdown();
}

/// A rule the lawyer kept that the file doesn't hold (its write failed, or is still pending) is
/// still removed from the running agent: the removal forgets it whether or not the file held it.
#[test]
fn a_removed_rule_the_file_lost_leaves_the_running_agent() {
    let dir = Dir::new(
        "kept_unpersisted",
        &[(CLAUDE, Scenario::Local("claude-always-remove"))],
    );
    let host = dir.start(0);
    let (matter, thread, _folder) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Run the tests.");
    answer_from(&host, &mut events, thread, 0, &["allow-with-updates"]);
    events.until_turn_ended(turn);
    // Once its write is done, the file loses the rule, as if the write had failed.
    let file = dir
        .data()
        .join("agent-state")
        .join(matter.to_string())
        .join("claude-allow.json");
    let stored = || std::fs::read_to_string(&file).is_ok_and(|text| text.contains("npm test"));
    crate::support::wait("the kept rule", async {
        while !stored() {
            async_io::Timer::after(std::time::Duration::from_millis(20)).await;
        }
    });
    keep(&dir, matter, &[]);
    let remove = HostCommand::RemoveKeptPermission {
        matter,
        rule: "Bash(npm test)".to_owned(),
    };
    assert_eq!(host.ok(remove), CommandOutcome::Done);
    let (turn, _) = host.prompt(thread, "Run them again.");
    answer_from(&host, &mut events, thread, 1, &["reject"]);
    events.until_turn_ended(turn);
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn an_always_that_keeps_nothing_says_it_was_allowed_once_only() {
    let dir = Dir::new(
        "kept_not_kept",
        &[(CLAUDE, Scenario::Local("claude-always-not-kept"))],
    );
    let host = dir.start(0);
    let (matter, thread, _folder) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "List the Word files.");
    answer_from(
        &host,
        &mut events,
        thread,
        0,
        &["allow-with-updates", "allow-with-updates"],
    );
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let said: Vec<(NoticeLevel, Option<NoticeKind>)> = notices(&events)
        .into_iter()
        .map(|notice| {
            assert!(
                notice.text.starts_with("Allowed this once only: "),
                "{notice:?}"
            );
            (notice.level, notice.kind)
        })
        .collect();
    let once = |reason| {
        (
            NoticeLevel::Info,
            Some(NoticeKind::AlwaysNotKept { reason }),
        )
    };
    assert_eq!(
        said,
        [once(NotKeptReason::NoRule), once(NotKeptReason::Refused)],
        "{}",
        events.dump()
    );
    assert_eq!(kept(&host, matter), Vec::new(), "nothing was kept");
    host.shutdown();
}
