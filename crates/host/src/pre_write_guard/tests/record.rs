//! The guard's record of its refusals (`record.rs`): a refusal the lawyer is told about adds one
//! line to the `--record` file the host made, with the `--record-id` identity it read from it, a
//! tool it lets go adds none, and a file it cannot write never changes the refusal. A hard link,
//! a symlink, a file moved in at the record's place, or a link at its folder's place (standing for
//! a shell profile, here a temp file) is never written through, and no file is ever created. The
//! host's `O_EXCL` refuses anything already at the record's place.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::{Fixture, deny, guard_with};
use clauseo_protocol::MatterPath;

use crate::pre_write_guard::record::{self, Agent, Reason, Record};
use crate::write::{self, RecordId};

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

/// The record file in the fixture's `guard` folder, beside the matter, made empty as the host
/// makes it, and the arguments naming it and its identity.
fn record_file(fx: &Fixture) -> (PathBuf, Vec<String>) {
    let folder = PathBuf::from(fx.path("guard"));
    write::create_dir_all(&folder).unwrap();
    let file = record::file(&folder);
    let id = write::create_record(&file).unwrap();
    let args = args(&file, id);
    (file, args)
}

/// The guard's arguments for the record `file` with the identity `id`.
fn args(file: &Path, id: RecordId) -> Vec<String> {
    vec![
        "--record".to_owned(),
        file.to_str().unwrap().to_owned(),
        "--record-id".to_owned(),
        id.hex(),
    ]
}

/// `args` as the guard takes them.
fn strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// A Codex patch updating `file`.
fn patch(file: &str) -> serde_json::Value {
    json!({"command": format!("*** Begin Patch\n*** Update File: {file}\n@@\n-a\n+b\n*** End Patch")})
}

#[test]
fn a_refused_write_to_a_held_file_adds_one_line_naming_it() {
    let fx = Fixture::new("record-held");
    let (file, args) = record_file(&fx);
    let hook = fx.hook("apply_patch", &patch("Contract.docx")).to_string();
    assert_eq!(
        guard_with(&strs(&args), hook.as_bytes()),
        deny("Contract.docx")
    );
    let bash = fx.hook("Bash", &json!({"command": "echo x >> Contract.docx"}));
    let matter = fx.matter();
    let mut claude = vec!["--matter", matter.to_str().unwrap()];
    claude.extend(strs(&args));
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
    let (file, args) = record_file(&fx);
    let hook = fx.hook(
        "apply_patch",
        &json!({"command":
        "*** Begin Patch\n*** Add File: .codex/config.toml\n+x\n*** End Patch"}),
    );
    let said = guard_with(&strs(&args), hook.to_string().as_bytes());
    assert!(said.contains(".codex or .agents folder"), "{said}");
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
    let (file, args) = record_file(&fx);
    for input in [
        fx.hook("apply_patch", &patch("Free.docx")),
        fx.hook("Bash", &json!({"command": "ls"})),
        fx.hook("Read", &json!({"file_path": "Contract.docx"})),
    ] {
        assert_eq!(guard_with(&strs(&args), input.to_string().as_bytes()), "");
    }
    let serve = fx.hook("Bash", &json!({"command": "stetdoc mcp"}));
    assert_ne!(guard_with(&strs(&args), serve.to_string().as_bytes()), "");
    assert_eq!(std::fs::read(&file).unwrap(), b"", "nothing was recorded");
}

/// No file there, a folder, a relative path, and the record the host made named with no
/// identity, a malformed one or another file's: the refusal stands, no file is made and the
/// record stays empty.
#[test]
fn a_record_file_it_cannot_write_never_changes_the_refusal() {
    let fx = Fixture::new("record-unwritable");
    let hook = fx.hook("apply_patch", &patch("Contract.docx")).to_string();
    let (made, made_args) = record_file(&fx);
    let id = made_args[3].clone();
    let missing = PathBuf::from(fx.path("no-such-folder")).join("blocks.jsonl");
    // No file there: the guard never makes one, the host does.
    let unmade = PathBuf::from(fx.path("unmade.jsonl"));
    let folder = PathBuf::from(fx.path("out"));
    write::create_dir_all(&folder).unwrap();
    for file in [&missing, &unmade, &folder] {
        let args = ["--record", file.to_str().unwrap(), "--record-id", &id];
        assert_eq!(
            guard_with(&args, hook.as_bytes()),
            deny("Contract.docx"),
            "{file:?}"
        );
    }
    assert!(!missing.exists());
    assert!(!unmade.exists(), "the guard made no file");
    let short = id.get(1..).unwrap();
    let other = write::create_record(&PathBuf::from(fx.path("other.jsonl"))).unwrap();
    let made = made.to_str().unwrap();
    for args in [
        vec!["--record", "blocks.jsonl", "--record-id", &id],
        vec!["--record", made],
        vec!["--record", made, "--record-id"],
        vec!["--record", made, "--record-id", "not hex"],
        vec!["--record", made, "--record-id", short],
        vec!["--record", made, "--record-id", &other.hex()],
    ] {
        assert_eq!(
            guard_with(&args, hook.as_bytes()),
            deny("Contract.docx"),
            "{args:?}"
        );
    }
    assert_eq!(std::fs::read(made).unwrap(), b"", "nothing was recorded");
}

/// A file outside the record's place with `text` in it: a shell profile, say.
fn elsewhere(fx: &Fixture, text: &[u8]) -> PathBuf {
    let target = PathBuf::from(fx.path("profile.txt"));
    write::write_file(&target, text).unwrap();
    target
}

/// The guard refuses the held file with `args` naming the record and its identity, and
/// `target` keeps `text`, untouched.
fn refused_and_untouched(fx: &Fixture, args: &[String], target: &Path, text: &[u8]) {
    let hook = fx.hook("apply_patch", &patch("Contract.docx")).to_string();
    assert_eq!(
        guard_with(&strs(args), hook.as_bytes()),
        deny("Contract.docx")
    );
    assert_eq!(
        std::fs::read(target).unwrap(),
        text,
        "written through {args:?}"
    );
}

/// The record the host made, taken away from its place (as an agent able to write its folder
/// would), with the arguments that still name it and its identity.
fn record_taken_away(fx: &Fixture) -> (PathBuf, Vec<String>) {
    let (file, args) = record_file(fx);
    write::rename(&file, &PathBuf::from(fx.path("taken-away.jsonl"))).unwrap();
    (file, args)
}

const PROFILE: &[u8] = b"export PATH=/usr/bin\n";

#[test]
fn a_hard_link_at_the_records_place_is_never_written_through() {
    let fx = Fixture::new("record-hard-link");
    let target = elsewhere(&fx, PROFILE);
    let (record, args) = record_taken_away(&fx);
    write::hard_link(&target, &record).unwrap();
    refused_and_untouched(&fx, &args, &target, PROFILE);
}

/// A regular file with one link moved in at the record's place, the record moved away, is
/// never written: it is not the file the host made (its identity differs), though the link
/// count and the open alone would let it through.
#[test]
fn a_file_moved_in_at_the_records_place_is_never_written() {
    let fx = Fixture::new("record-moved-in");
    let planted = elsewhere(&fx, PROFILE);
    let (record, args) = record_taken_away(&fx);
    write::rename(&planted, &record).unwrap();
    refused_and_untouched(&fx, &args, &record, PROFILE);
    assert_eq!(std::fs::read(fx.path("taken-away.jsonl")).unwrap(), b"");
}

/// A symlink at the record's place (`O_NOFOLLOW`), and a symlink at its folder's place to a
/// folder holding a profile under the record's name: that one is opened, since `O_NOFOLLOW`
/// looks at the last part of the path only, and refused for its identity, read from the opened
/// file.
#[cfg(unix)]
#[test]
fn a_symlink_at_the_records_place_is_never_written_through() {
    let fx = Fixture::new("record-symlink");
    let target = elsewhere(&fx, PROFILE);
    let (record, args) = record_taken_away(&fx);
    write::symlink(&target, &record).unwrap();
    refused_and_untouched(&fx, &args, &target, PROFILE);

    let folder = record.parent().unwrap();
    let outside = PathBuf::from(fx.path("outside"));
    write::create_dir_all(&outside).unwrap();
    let planted = record::file(&outside);
    write::write_file(&planted, PROFILE).unwrap();
    write::rename(folder, &PathBuf::from(fx.path("guard.old"))).unwrap();
    write::symlink(&outside, folder).unwrap();
    refused_and_untouched(&fx, &args, &planted, PROFILE);
}

/// The host's `O_EXCL` refuses a file, a file symlink or a dangling symlink already at the
/// record's place: `AlreadyExists`, nothing written through and no file made at a link's target.
#[cfg(unix)]
#[test]
fn creating_the_record_refuses_a_file_or_a_symlink_already_there() {
    let fx = Fixture::new("record-create");
    let target = elsewhere(&fx, PROFILE);
    let file = PathBuf::from(fx.path("file.jsonl"));
    write::write_file(&file, PROFILE).unwrap();
    let link = PathBuf::from(fx.path("link.jsonl"));
    write::symlink(&target, &link).unwrap();
    let dangling = PathBuf::from(fx.path("dangling.jsonl"));
    let nowhere = PathBuf::from(fx.path("nowhere.txt"));
    write::symlink(&nowhere, &dangling).unwrap();
    for place in [&file, &link, &dangling] {
        let error = write::create_record(place).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists, "{place:?}");
    }
    assert_eq!(std::fs::read(&file).unwrap(), PROFILE);
    assert_eq!(std::fs::read(&target).unwrap(), PROFILE);
    assert!(!nowhere.exists(), "no file made through the dangling link");
}

/// Makes the file symlink `link` to `target`, or says why not. Windows makes one only with
/// Developer Mode or an elevated token: without either the test is skipped with a printed
/// reason (`false`), unless `CLAUSEO_TEST_REQUIRE_SYMLINKS=1` (CI's Developer Mode runner),
/// where it fails.
#[cfg(windows)]
fn symlink_or_skip(target: &Path, link: &Path) -> bool {
    match write::symlink_file(target, link) {
        Ok(()) => true,
        Err(error) if std::env::var("CLAUSEO_TEST_REQUIRE_SYMLINKS").as_deref() == Ok("1") => {
            panic!("CLAUSEO_TEST_REQUIRE_SYMLINKS=1 but no symlink could be made: {error}")
        }
        Err(error) => {
            eprintln!("skipped: no right to make a symlink here ({error})");
            false
        }
    }
}

/// Windows: a file symlink at the record's place (`FILE_FLAG_OPEN_REPARSE_POINT`); the hard
/// link test above still covers the link count when this one is skipped.
#[cfg(windows)]
#[test]
fn a_symlink_at_the_records_place_is_never_written_through() {
    let fx = Fixture::new("record-symlink");
    let target = elsewhere(&fx, PROFILE);
    let (record, args) = record_taken_away(&fx);
    if !symlink_or_skip(&target, &record) {
        return;
    }
    refused_and_untouched(&fx, &args, &target, PROFILE);
}

/// Windows: a junction, which any user may make, at the record's folder's place, to a folder
/// holding a profile under the record's name. `FILE_FLAG_OPEN_REPARSE_POINT` looks at the last
/// part of the path only, so it is opened, and refused for its identity.
#[cfg(windows)]
#[test]
fn a_junction_at_the_records_folder_is_never_written_through() {
    let fx = Fixture::new("record-junction");
    let (record, args) = record_taken_away(&fx);
    let folder = record.parent().unwrap();
    let outside = PathBuf::from(fx.path("outside"));
    write::create_dir_all(&outside).unwrap();
    let planted = record::file(&outside);
    write::write_file(&planted, PROFILE).unwrap();
    write::rename(folder, &PathBuf::from(fx.path("guard.old"))).unwrap();
    let outside = dunce::canonicalize(&outside).unwrap();
    clauseo_sandbox::windows::test_junction::junction(folder, &outside).unwrap();
    refused_and_untouched(&fx, &args, &planted, PROFILE);
}

/// Windows: the host's create-new refuses a file symlink or a dangling one at the record's
/// place, and makes no file at the link's target.
#[cfg(windows)]
#[test]
fn creating_the_record_refuses_a_symlink_already_there() {
    let fx = Fixture::new("record-create");
    let target = elsewhere(&fx, PROFILE);
    let link = PathBuf::from(fx.path("link.jsonl"));
    if !symlink_or_skip(&target, &link) {
        return;
    }
    let dangling = PathBuf::from(fx.path("dangling.jsonl"));
    let nowhere = PathBuf::from(fx.path("nowhere.txt"));
    assert!(symlink_or_skip(&nowhere, &dangling));
    for place in [&link, &dangling] {
        let error = write::create_record(place).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists, "{place:?}");
    }
    assert_eq!(std::fs::read(&target).unwrap(), PROFILE);
    assert!(!nowhere.exists(), "no file made through the dangling link");
}
