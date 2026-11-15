//! The guard's record of its refusals (`record.rs`): a refusal the lawyer is told about adds one
//! line to the `--record` file, a tool it lets go adds none, and a file it cannot write never
//! changes the refusal.

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
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The record file in the fixture's root, beside the matter.
fn record_file(fx: &Fixture) -> PathBuf {
    PathBuf::from(fx.path("guard-blocks.jsonl"))
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
    assert!(!file.exists(), "nothing was recorded");
}

#[test]
fn a_record_file_it_cannot_write_never_changes_the_refusal() {
    let fx = Fixture::new("record-unwritable");
    let hook = fx.hook("apply_patch", &patch("Contract.docx")).to_string();
    let missing = PathBuf::from(fx.path("no-such-folder")).join("blocks.jsonl");
    let folder = PathBuf::from(fx.path("out"));
    let mut files = vec![missing.clone(), folder];
    #[cfg(unix)]
    {
        // A link at the record's place is never followed, and the file it names stays empty.
        let target = PathBuf::from(fx.path("target.jsonl"));
        write::write_file(&target, b"").unwrap();
        let link = PathBuf::from(fx.path("link.jsonl"));
        write::symlink(&target, &link).unwrap();
        files.push(link);
    }
    for file in &files {
        let args = ["--record", file.to_str().unwrap()];
        assert_eq!(
            guard_with(&args, hook.as_bytes()),
            deny("Contract.docx"),
            "{file:?}"
        );
    }
    assert!(!missing.exists());
    #[cfg(unix)]
    assert_eq!(std::fs::read(fx.path("target.jsonl")).unwrap(), b"");
    // A relative file is no record file at all.
    assert_eq!(
        guard_with(&["--record", "blocks.jsonl"], hook.as_bytes()),
        deny("Contract.docx")
    );
}
