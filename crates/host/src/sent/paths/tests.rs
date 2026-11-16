//! Which places of a turn's tool calls count as written ([`TurnPaths::written`]).

use clauseo_protocol::{
    MatterPath, TextOrBlob, ToolCallContent, ToolCallStatus, ToolKind, ToolLocation,
};

use super::{CallFields, Places, ShellStart, TurnPaths};

fn inside(path: &str) -> ToolLocation {
    ToolLocation::Inside {
        path: MatterPath::new(path).unwrap(),
        line: None,
    }
}

fn diff(path: &str) -> ToolCallContent {
    ToolCallContent::Diff {
        path: inside(path),
        old: None,
        new: TextOrBlob::Inline("x".to_owned()),
    }
}

fn fields<'a>(
    kind: Option<ToolKind>,
    status: Option<ToolCallStatus>,
    locations: &'a [ToolLocation],
    content: &'a [ToolCallContent],
) -> CallFields<'a> {
    CallFields {
        kind,
        status,
        locations,
        content,
        raw_input: None,
        tool_name: None,
    }
}

fn matter(paths: &[&str]) -> Places {
    Places {
        inside: paths
            .iter()
            .map(|path| MatterPath::new(path).unwrap())
            .collect(),
        outside: std::collections::BTreeSet::new(),
    }
}

const DONE: Option<ToolCallStatus> = Some(ToolCallStatus::Completed);

/// A read's, a search's or a shell call's location is no write; an edit's, a delete's and a
/// move's are, and so is a diff's path whatever the call's kind.
#[test]
fn only_writing_kinds_and_diffs_count() {
    let mut paths = TurnPaths::default();
    let cases = [
        ("read", ToolKind::Read),
        ("search", ToolKind::Search),
        ("fetch", ToolKind::Fetch),
        ("think", ToolKind::Think),
        ("execute", ToolKind::Execute),
        ("other", ToolKind::Other),
        ("edit", ToolKind::Edit),
        ("delete", ToolKind::Delete),
        ("move", ToolKind::Move),
    ];
    for (key, kind) in cases {
        let location = [inside(&format!("{key}.txt"))];
        paths.note(key, &fields(Some(kind), DONE, &location, &[]));
    }
    let diffed = [diff("read-diff.txt")];
    paths.note(
        "read-diff",
        &fields(Some(ToolKind::Read), DONE, &[], &diffed),
    );
    assert_eq!(
        paths.written(),
        matter(&["delete.txt", "edit.txt", "move.txt", "read-diff.txt"])
    );
}

/// An update with no kind keeps the start's: a read's later location is still no write, an
/// edit's is; an update of a call never seen start counts by its diffs alone.
#[test]
fn an_update_takes_the_kind_its_call_started_with() {
    let mut paths = TurnPaths::default();
    paths.note("r", &fields(Some(ToolKind::Read), None, &[], &[]));
    paths.note("e", &fields(Some(ToolKind::Edit), None, &[], &[]));
    paths.note("r", &fields(None, DONE, &[inside("read.txt")], &[]));
    paths.note("e", &fields(None, DONE, &[inside("edit.txt")], &[]));
    let diffed = [diff("diffed.txt")];
    paths.note("u", &fields(None, DONE, &[inside("unseen.txt")], &diffed));
    assert_eq!(paths.written(), matter(&["diffed.txt", "edit.txt"]));
}

/// A call that ended failed wrote nothing it named; one that failed and then completed did.
#[test]
fn a_failed_call_writes_nothing() {
    let mut paths = TurnPaths::default();
    let failed = Some(ToolCallStatus::Failed);
    let diffed = [diff("failed.txt")];
    paths.note("f", &fields(Some(ToolKind::Edit), None, &[], &diffed));
    paths.note("f", &fields(None, failed, &[], &[]));
    paths.note(
        "g",
        &fields(Some(ToolKind::Edit), failed, &[inside("later.txt")], &[]),
    );
    paths.note("g", &fields(None, DONE, &[], &[]));
    assert_eq!(paths.written(), matter(&["later.txt"]));
}

/// A shell call's command is read for the files it writes, its later updates too; another
/// kind's input is not.
#[test]
fn a_shell_calls_command_is_read_for_writes() {
    let mut paths = TurnPaths::default();
    let input = TextOrBlob::Inline(r#"{"command":"echo x > out.txt"}"#.to_owned());
    let mut shell = fields(Some(ToolKind::Execute), None, &[], &[]);
    shell.raw_input = Some(&input);
    paths.note("s", &shell);
    let later = TextOrBlob::Inline(r#"{"command":"echo y > later.txt"}"#.to_owned());
    let mut update = fields(None, DONE, &[], &[]);
    update.raw_input = Some(&later);
    paths.note("s", &update);
    let mut read = fields(Some(ToolKind::Read), None, &[], &[]);
    read.raw_input = Some(&input);
    paths.note("r", &read);
    assert_eq!(
        paths.shell_writes(),
        ["later.txt".to_owned(), "out.txt".to_owned()].into()
    );
    assert_eq!(paths.written(), Places::default());
}

/// A shell call's targets count only once it ends `Completed`: a denied, failed or cancelled
/// command, or one still running at the checkpoint, wrote nothing.
#[test]
fn only_a_completed_shell_call_writes() {
    let mut paths = TurnPaths::default();
    let statuses = [
        ("done", Some(ToolCallStatus::Completed)),
        ("failed", Some(ToolCallStatus::Failed)),
        ("cancelled", Some(ToolCallStatus::Cancelled)),
        ("pending", Some(ToolCallStatus::Pending)),
        ("running", Some(ToolCallStatus::InProgress)),
        ("unknown", None),
    ];
    let inputs: Vec<TextOrBlob> = statuses
        .iter()
        .map(|(key, _)| TextOrBlob::Inline(format!(r#"{{"command":"pandoc a.md -o {key}.docx"}}"#)))
        .collect();
    for ((key, status), input) in statuses.iter().zip(&inputs) {
        let mut start = fields(
            Some(ToolKind::Execute),
            Some(ToolCallStatus::Pending),
            &[],
            &[],
        );
        start.raw_input = Some(input);
        paths.note(key, &start);
        paths.note(key, &fields(None, *status, &[], &[]));
    }
    assert_eq!(paths.shell_writes(), ["done.docx".to_owned()].into());
    // A failure after a completion (a late update) takes the write back.
    paths.note(
        "done",
        &fields(None, Some(ToolCallStatus::Failed), &[], &[]),
    );
    assert!(paths.shell_writes().is_empty());
}

/// An edit or a diff of a call that has not ended `Completed` (an ask still open at a Stop, a
/// cancelled call) wrote nothing.
#[test]
fn only_a_completed_edit_writes() {
    let memo = [inside("memo.docx")];
    let diffed = [diff("memo.docx")];
    for status in [
        None,
        Some(ToolCallStatus::Pending),
        Some(ToolCallStatus::InProgress),
        Some(ToolCallStatus::Cancelled),
        Some(ToolCallStatus::Failed),
    ] {
        let mut paths = TurnPaths::default();
        paths.note("e", &fields(Some(ToolKind::Edit), status, &memo, &diffed));
        assert_eq!(paths.written(), Places::default(), "{status:?}");
    }
    let mut paths = TurnPaths::default();
    paths.note("e", &fields(Some(ToolKind::Edit), DONE, &memo, &diffed));
    assert_eq!(paths.written(), matter(&["memo.docx"]));
}

/// Codex's exec runs each call in its own process, from its `cwd` (the turn's) else the
/// matter's folder: one call's `cd` never moves where the next writes, nor does Claude's under
/// `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR` (`crate::agent::loads`).
#[test]
fn each_shell_call_starts_in_its_own_folder() {
    let mut paths = TurnPaths::default();
    for (key, command) in [("1", "cd Sub"), ("2", "pandoc a.md -o x.docx")] {
        let input = TextOrBlob::Inline(
            serde_json::json!({ "command": command, "cwd": "/matters/Deal" }).to_string(),
        );
        let mut call = fields(Some(ToolKind::Execute), DONE, &[], &[]);
        call.raw_input = Some(&input);
        paths.note(key, &call);
    }
    for (key, command) in [("3", "cd Sub"), ("4", "echo x > y.md")] {
        let input = TextOrBlob::Inline(serde_json::json!({ "command": command }).to_string());
        let mut call = fields(Some(ToolKind::Execute), DONE, &[], &[]);
        call.raw_input = Some(&input);
        paths.note(key, &call);
    }
    assert_eq!(
        paths.shell_writes(),
        ["/matters/Deal/x.docx".to_owned(), "y.md".to_owned()].into()
    );
}

/// Where the turn's shell calls start: a relative target counts only from the matter's folder;
/// a repair's policy start is read from the policy settings in its folder.
#[test]
fn a_shell_call_from_an_unknown_start_counts_no_relative_target() {
    let input = TextOrBlob::Inline(r#"{"command":"echo x > a.md > /m/b.md"}"#.to_owned());
    let mut call = fields(Some(ToolKind::Execute), DONE, &[], &[]);
    call.raw_input = Some(&input);
    let mut paths = TurnPaths::default();
    paths.note("s", &call);
    assert_eq!(
        paths.shell_writes(),
        ["/m/b.md".to_owned(), "a.md".to_owned()].into()
    );
    paths.set_start(ShellStart::Unknown);
    assert_eq!(paths.shell_writes(), ["/m/b.md".to_owned()].into());
    let managed = std::env::temp_dir().join(format!("clauseo-paths-policy-{}", std::process::id()));
    crate::write::remove_dir_all(&managed).unwrap();
    paths.set_start(ShellStart::Policy(managed.clone()));
    paths.resolve_start();
    assert_eq!(
        paths.shell_writes().len(),
        2,
        "no policy file: the matter's folder"
    );
    crate::write::create_dir_all(&managed).unwrap();
    crate::write::write_file(
        &managed.join("managed-settings.json"),
        br#"{"env": {"CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR": "0"}}"#,
    )
    .unwrap();
    paths.set_start(ShellStart::Policy(managed.clone()));
    paths.resolve_start();
    assert_eq!(paths.shell_writes(), ["/m/b.md".to_owned()].into());
    crate::write::remove_dir_all(&managed).unwrap();
}
