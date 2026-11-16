//! The guard's record of its refusals (`record.rs`): a refusal the lawyer is told about adds one
//! line to the `--record` file the host made, a tool it lets go adds none, and a file it cannot
//! write never changes the refusal. A hard link or a symlink at the record's place (standing for
//! a shell profile, here a temp file) is never written through, and no file is ever created.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::{Fixture, deny, guard_with};
use clauseo_protocol::MatterPath;

use crate::pre_write_guard::record::{Agent, Reason, Record};
use crate::write;

/// The lines of the record file `file`, parsed; none when it is not there.
fn records(file: &Path) -> Vec<Record> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The record file in the fixture's root, beside the matter, made empty as the host makes it.
fn record_file(fx: &Fixture) -> PathBuf {
    let file = PathBuf::from(fx.path("guard-blocks.jsonl"));
    write::create_record(&file).unwrap();
    file
}

/// A Codex patch updating `file`.
fn patch(file: &str) -> serde_json::Value {
    json!({"command": format!("*** Begin Patch\n*** Update File: {file}\n@@\n-a\n+b\n*** End Patch")})
}

#[test]
fn a_refused_write_to_a_held_file_adds_one_line_naming_it() {
    let fx = Fixture::new("record-held");
    let file = record_file(&fx);
    let path = file.to_str().unwrap();
    let hook = fx.hook("apply_patch", &patch("Contract.docx")).to_string();
    assert_eq!(
        guard_with(&["--record", path], hook.as_bytes()),
        deny("Contract.docx")
    );
    let bash = fx.hook("Bash", &json!({"command": "echo x >> Contract.docx"}));
    let matter = fx.matter();
    let claude = ["--matter", matter.to_str().unwrap(), "--record", path];
    assert_eq!(
        guard_with(&claude, bash.to_string().as_bytes()),
        deny("Contract.docx")
    );
    let lines = records(&file);
    assert_eq!(lines.len(), 2, "{lines:?}");
    for (line, (agent, tool)) in lines
        .iter()
        .zip([(Agent::Codex, "apply_patch"), (Agent::Claude, "Bash")])
    {
        assert_eq!(line.agent, agent);
        assert_eq!(line.tool, tool);
        assert_eq!(line.reason, Reason::HeldOpen);
        assert_eq!(
            line.path.as_ref().map(MatterPath::as_str),
            Some("Contract.docx")
        );
        assert!(line.at.as_millis() > 0, "{line:?}");
    }
}

#[test]
fn a_refused_write_into_the_codex_folder_adds_a_line_with_no_path() {
    let fx = Fixture::new("record-codex");
    let file = record_file(&fx);
    let hook = fx.hook(
        "apply_patch",
        &json!({"command":
        "*** Begin Patch\n*** Add File: .codex/config.toml\n+x\n*** End Patch"}),
    );
    let said = guard_with(
        &["--record", file.to_str().unwrap()],
        hook.to_string().as_bytes(),
    );
    assert!(said.contains(".codex folder"), "{said}");
    let lines = records(&file);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        (lines[0].reason, lines[0].path.clone()),
        (Reason::CodexFolder, None)
    );
    assert_eq!(
        (lines[0].agent, lines[0].tool.as_str()),
        (Agent::Codex, "apply_patch")
    );
}

#[test]
fn a_tool_let_through_or_a_refusal_the_lawyer_is_not_told_adds_nothing() {
    let fx = Fixture::new("record-none");
    let file = record_file(&fx);
    let path = file.to_str().unwrap();
    for input in [
        fx.hook("apply_patch", &patch("Free.docx")),
        fx.hook("Bash", &json!({"command": "ls"})),
        fx.hook("Read", &json!({"file_path": "Contract.docx"})),
    ] {
        assert_eq!(
            guard_with(&["--record", path], input.to_string().as_bytes()),
            ""
        );
    }
    let serve = fx.hook("Bash", &json!({"command": "stetdoc mcp"}));
    assert_ne!(
        guard_with(&["--record", path], serve.to_string().as_bytes()),
        ""
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"", "nothing was recorded");
}

#[test]
fn a_record_file_it_cannot_write_never_changes_the_refusal() {
    let fx = Fixture::new("record-unwritable");
    let hook = fx.hook("apply_patch", &patch("Contract.docx")).to_string();
    let missing = PathBuf::from(fx.path("no-such-folder")).join("blocks.jsonl");
    // No file there: the guard never makes one, the host does.
    let unmade = PathBuf::from(fx.path("unmade.jsonl"));
    let folder = PathBuf::from(fx.path("out"));
    for file in [&missing, &unmade, &folder] {
        let args = ["--record", file.to_str().unwrap()];
        assert_eq!(
            guard_with(&args, hook.as_bytes()),
            deny("Contract.docx"),
            "{file:?}"
        );
    }
    assert!(!missing.exists());
    assert!(!unmade.exists(), "the guard made no file");
    // A relative file is no record file at all.
    assert_eq!(
        guard_with(&["--record", "blocks.jsonl"], hook.as_bytes()),
        deny("Contract.docx")
    );
}

/// A file outside the record's place with `text` in it: a shell profile, say.
fn elsewhere(fx: &Fixture, text: &[u8]) -> PathBuf {
    let target = PathBuf::from(fx.path("profile.txt"));
    write::write_file(&target, text).unwrap();
    target
}

/// The guard refuses the held file with `record` as its record file, and `target` keeps
/// `text`, untouched.
fn refused_and_untouched(fx: &Fixture, record: &Path, target: &Path, text: &[u8]) {
    let hook = fx.hook("apply_patch", &patch("Contract.docx")).to_string();
    let args = ["--record", record.to_str().unwrap()];
    assert_eq!(guard_with(&args, hook.as_bytes()), deny("Contract.docx"));
    assert_eq!(
        std::fs::read(target).unwrap(),
        text,
        "written through {record:?}"
    );
}

#[test]
fn a_hard_link_at_the_records_place_is_never_written_through() {
    let fx = Fixture::new("record-hard-link");
    let text = b"export PATH=/usr/bin\n";
    let target = elsewhere(&fx, text);
    let record = PathBuf::from(fx.path("guard-blocks.jsonl"));
    write::hard_link(&target, &record).unwrap();
    refused_and_untouched(&fx, &record, &target, text);
}

#[cfg(unix)]
#[test]
fn a_symlink_at_the_records_place_is_never_written_through() {
    let fx = Fixture::new("record-symlink");
    let text = b"export PATH=/usr/bin\n";
    let target = elsewhere(&fx, text);
    let record = PathBuf::from(fx.path("guard-blocks.jsonl"));
    write::symlink(&target, &record).unwrap();
    refused_and_untouched(&fx, &record, &target, text);
}

/// Windows: a file symlink at the record's place. Making one needs Developer Mode or an
/// elevated token; without either the test says so and passes, the hard link test above still
/// covering the link count. A junction links folders only, so the record's place cannot be one
/// that opens as a file.
#[cfg(windows)]
#[test]
fn a_symlink_at_the_records_place_is_never_written_through() {
    let fx = Fixture::new("record-symlink");
    let text = b"export PATH=/usr/bin\n";
    let target = elsewhere(&fx, text);
    let record = PathBuf::from(fx.path("guard-blocks.jsonl"));
    if let Err(error) = write::symlink_file(&target, &record) {
        eprintln!("skipped: no right to make a symlink here ({error})");
        return;
    }
    refused_and_untouched(&fx, &record, &target, text);
}
