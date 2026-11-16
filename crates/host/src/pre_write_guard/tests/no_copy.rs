//! The no-copy rule (`no_copy.rs`): a delete of a file the session's list names is refused by every
//! form the guard reads, with the reason, and recorded for the lawyer; an overwrite, an edit or a
//! move is refused only for a file that is cloud-only or over the size cap now (a listed file the
//! agent's read downloaded may be edited); a read or a write elsewhere goes on. The list is trusted
//! only as the regular file with one link the host made (its identity in the arguments): a hard
//! link, a symlink, another file, a missing one or none at all reads as an empty list, and the
//! guard still refuses a file it can see has no copy itself (here one over the size cap), so a bad
//! list never opens the gate.

use std::path::{Path, PathBuf};

use clauseo_matters::FILE_CAP_BYTES;
use clauseo_protocol::{MatterPath, SkippedFileReason};
use serde_json::{Value, json};

use super::{Fixture, deny, guard_with};
use crate::pre_write_guard::no_copy::{Act, HEADER, reason};
use crate::pre_write_guard::record::{Reason, Record};
use crate::write;

const CLOUD: SkippedFileReason = SkippedFileReason::CloudOnly;
const LARGE: SkippedFileReason = SkippedFileReason::OverSizeCap;

/// The list Free.docx (cloud-only) and sub/Note.txt (over the cap) are on.
const LINES: &str = "add cloud-only Free.docx\nadd over-size-cap sub/Note.txt\n";

/// The list at the fixture's `no-copy.txt` holding `lines`, made as the host makes it; its path
/// and identity.
fn list(fx: &Fixture, lines: &str) -> (PathBuf, String) {
    let file = PathBuf::from(fx.path("no-copy.txt"));
    let id = write::create_record(&file).unwrap();
    let text = format!("{HEADER}{lines}");
    std::io::Write::write_all(
        &mut write::open_append_record(&file, id).unwrap(),
        text.as_bytes(),
    )
    .unwrap();
    (file, id.hex())
}

/// The identity of the file at `path`.
fn id_of(path: &Path) -> String {
    write::RecordId::of(&std::fs::File::open(path).unwrap())
        .unwrap()
        .hex()
}

/// The guard's arguments for the list `file` with `id`, after `before`.
fn args<'a>(before: &[&'a str], file: &'a Path, id: &'a str) -> Vec<&'a str> {
    let mut args = before.to_vec();
    args.extend([
        "--no-copy-list",
        file.to_str().unwrap(),
        "--no-copy-list-id",
        id,
    ]);
    args
}

/// The exact stderr line refusing a delete of `path` for `why`.
fn refused(path: &str, why: SkippedFileReason) -> String {
    format!(
        "{}\n",
        reason(&MatterPath::new(path).unwrap(), why, Act::Delete)
    )
}

/// The exact stderr line refusing an overwrite of `path` for `why`.
fn refused_overwrite(path: &str, why: SkippedFileReason) -> String {
    format!(
        "{}\n",
        reason(&MatterPath::new(path).unwrap(), why, Act::Overwrite)
    )
}

/// A Codex patch with the one file line `line`.
fn patch(line: &str) -> Value {
    json!({"command": format!("*** Begin Patch\n{line}\n@@\n-a\n+b\n*** End Patch")})
}

/// `m/Big.bin`, one byte over the cap (sparse: no data written).
fn big(fx: &Fixture) {
    write::sized_file(&PathBuf::from(fx.path("m/Big.bin")), FILE_CAP_BYTES + 1).unwrap();
}

#[test]
fn a_listed_file_is_refused_by_every_delete_form_the_guard_reads() {
    let fx = Fixture::new("no-copy-forms");
    let (file, id) = list(&fx, LINES);
    let matter = fx.matter();
    let claude = args(&["--matter", matter.to_str().unwrap()], &file, &id);
    let codex = args(&[], &file, &id);
    let free = || refused("Free.docx", CLOUD);
    let note = || refused("sub/Note.txt", LARGE);
    let hook = fx.hook("apply_patch", &patch("*** Delete File: Free.docx"));
    assert_eq!(guard_with(&codex, hook.to_string().as_bytes()), free());
    for (command, said) in [
        ("rm Free.docx", free()),
        ("rm -f \"Free.docx\"", free()),
        ("unlink ./Free.docx", free()),
        ("cd sub && rm Note.txt", note()),
        ("rm -rf sub", note()),
        ("rm -r sub/", note()),
        ("rm -rf .", free()),
        ("cd sub; rm -rf ..", free()),
        ("ls; rm \"$PWD/../m/Free.docx\" || true", String::new()),
        (
            "apply_patch <<'EOF'\n*** Begin Patch\n*** Delete File: Free.docx\n*** End Patch\nEOF",
            free(),
        ),
    ] {
        let hook = fx.hook("Bash", &json!({"command": command}));
        assert_eq!(
            guard_with(&codex, hook.to_string().as_bytes()),
            said,
            "{command}"
        );
    }
    let absolute = format!("rm '{}'", fx.path("m/Free.docx").replace('\\', "/"));
    let hook = fx.hook("Bash", &json!({"command": absolute}));
    assert_eq!(guard_with(&claude, hook.to_string().as_bytes()), free());
    for command in ["Remove-Item -Path Free.docx -Force", "del Free.docx"] {
        let hook = fx.hook("PowerShell", &json!({"command": command}));
        assert_eq!(
            guard_with(&claude, hook.to_string().as_bytes()),
            free(),
            "{command}"
        );
    }
}

/// R2: a listed file that is on the disk now and within the cap (here: every listed file of the
/// fixture, small local files) may be overwritten, edited or moved: the agent's read of a
/// cloud-only file downloads it, and its edit must not be refused for the rest of the turn.
#[test]
fn a_listed_file_on_the_disk_now_may_be_overwritten_or_moved() {
    let fx = Fixture::new("no-copy-local");
    let (file, id) = list(&fx, LINES);
    let matter = fx.matter();
    let claude = args(&["--matter", matter.to_str().unwrap()], &file, &id);
    let codex = args(&[], &file, &id);
    for tool in ["Write", "Edit", "MultiEdit"] {
        let hook = fx.hook(tool, &json!({"file_path": "Free.docx", "content": "x"}));
        assert_eq!(
            guard_with(&claude, hook.to_string().as_bytes()),
            "",
            "{tool}"
        );
    }
    let notebook = fx.hook("NotebookEdit", &json!({"notebook_path": "sub/Note.txt"}));
    assert_eq!(guard_with(&claude, notebook.to_string().as_bytes()), "");
    for line in [
        "*** Update File: Free.docx",
        "*** Update File: Other.docx\n*** Move to: Free.docx",
    ] {
        let hook = fx.hook("apply_patch", &patch(line));
        assert_eq!(
            guard_with(&codex, hook.to_string().as_bytes()),
            "",
            "{line}"
        );
    }
    for command in [
        "mv Free.docx Renamed.docx",
        "mv Other.docx Free.docx",
        "cp Other.docx Free.docx",
        "echo x > Free.docx",
        "echo x >> Free.docx",
        "echo x | tee Free.docx",
        "pandoc a.md -o Free.docx",
        "cd sub && echo x > Note.txt",
    ] {
        let hook = fx.hook("Bash", &json!({"command": command}));
        assert_eq!(
            guard_with(&codex, hook.to_string().as_bytes()),
            "",
            "{command}"
        );
    }
    let hook = fx.hook("PowerShell", &json!({"command": "Move-Item Free.docx x"}));
    assert_eq!(guard_with(&claude, hook.to_string().as_bytes()), "");
}

/// An overwrite, an edit or a move of a file over the size cap now (listed or not) is refused,
/// with the overwrite's words: write to a new file, and tell the lawyer.
#[test]
fn a_file_over_the_cap_now_is_refused_to_an_overwrite() {
    let fx = Fixture::new("no-copy-overwrite");
    big(&fx);
    let (file, id) = list(&fx, "add over-size-cap Big.bin\n");
    let matter = fx.matter();
    let claude = args(&["--matter", matter.to_str().unwrap()], &file, &id);
    let codex = args(&[], &file, &id);
    let said = refused_overwrite("Big.bin", LARGE);
    let hook = fx.hook("Write", &json!({"file_path": "Big.bin", "content": "x"}));
    assert_eq!(guard_with(&claude, hook.to_string().as_bytes()), said);
    let hook = fx.hook("apply_patch", &patch("*** Update File: Big.bin"));
    assert_eq!(guard_with(&codex, hook.to_string().as_bytes()), said);
    for command in ["cp a Big.bin", "echo x > Big.bin", "mv Big.bin Old.bin"] {
        let hook = fx.hook("Bash", &json!({"command": command}));
        assert_eq!(
            guard_with(&codex, hook.to_string().as_bytes()),
            said,
            "{command}"
        );
    }
    let hook = fx.hook("Bash", &json!({"command": "rm Big.bin"}));
    assert_eq!(
        guard_with(&codex, hook.to_string().as_bytes()),
        refused("Big.bin", LARGE)
    );
}

/// R4: the agent's words are true for each case and promise nothing for asking: a delete says
/// the lawyer can delete it themselves; an overwrite says to write to a new file; neither says a
/// listed file being deleted is "not downloaded".
#[test]
fn the_refusal_words_say_what_is_true_and_promise_nothing() {
    let path = MatterPath::new("Contract.docx").unwrap();
    let delete = reason(&path, CLOUD, Act::Delete);
    assert_eq!(
        delete,
        "Contract.docx can't be brought back if it is deleted: Clauseo keeps no copy of it, \
         because it was only online, not on this computer, when this answer began. Clauseo does \
         not let you delete it. Tell the lawyer; they can delete it themselves."
    );
    let overwrite = reason(&path, LARGE, Act::Overwrite);
    assert_eq!(
        overwrite,
        "Contract.docx can't be brought back if it is overwritten: Clauseo keeps no copy of it, \
         because it is larger than 512 MB. Write to a new file instead, and tell the lawyer."
    );
    for text in [&delete, &overwrite, &reason(&path, CLOUD, Act::Overwrite)] {
        assert!(!text.contains("Ask the lawyer"), "{text}");
        assert!(!text.contains("moved"), "{text}");
    }
    assert!(!delete.contains("not downloaded"));
}

/// F2: a matter kept on a network share is never touched (`network_paths::touchable` refuses
/// every path in it, so nothing is walked): the list is looked up by the path's text alone, and a
/// delete of a listed file is still refused there. `/network/…` is a network location by its text
/// (and, unlike macOS's `/net`, no automount: nothing there is ever looked up).
#[cfg(unix)]
#[test]
fn a_listed_file_in_a_matter_on_a_share_is_refused_by_its_text() {
    let fx = Fixture::new("no-copy-share");
    let (file, id) = list(&fx, LINES);
    let share = "/network/clauseo-no-copy-test/m";
    let claude = args(&["--matter", share], &file, &id);
    let hook = json!({
        "tool_name": "Bash",
        "tool_input": {"command": "rm Free.docx"},
        "cwd": share,
    });
    assert_eq!(
        guard_with(&claude, hook.to_string().as_bytes()),
        refused("Free.docx", CLOUD)
    );
    let hook = json!({
        "tool_name": "Bash",
        "tool_input": {"command": "rm -r sub"},
        "cwd": share,
    });
    assert_eq!(
        guard_with(&claude, hook.to_string().as_bytes()),
        refused("sub/Note.txt", LARGE)
    );
}

#[test]
fn a_read_or_a_write_elsewhere_goes_on() {
    let fx = Fixture::new("no-copy-reads");
    let (file, id) = list(&fx, LINES);
    let codex = args(&[], &file, &id);
    for command in [
        "cat Free.docx",
        "cp Free.docx Copy.docx",
        "pandoc Free.docx -o Summary.md",
        "mv Other.docx sub",
        "rm Other.docx",
        "grep -l x sub/Note.txt > found.txt",
    ] {
        let hook = fx.hook("Bash", &json!({"command": command}));
        assert_eq!(
            guard_with(&codex, hook.to_string().as_bytes()),
            "",
            "{command}"
        );
    }
    for (tool, input) in [
        ("Write", json!({"file_path": "New.docx", "content": "x"})),
        ("Read", json!({"file_path": "Free.docx"})),
        ("apply_patch", patch("*** Add File: New.docx")),
    ] {
        let hook = fx.hook(tool, &input);
        assert_eq!(
            guard_with(&codex, hook.to_string().as_bytes()),
            "",
            "{tool}"
        );
    }
}

#[test]
fn a_held_file_is_refused_as_held_first() {
    let fx = Fixture::new("no-copy-held");
    let (file, id) = list(&fx, "add cloud-only Contract.docx\n");
    let hook = fx.hook("Bash", &json!({"command": "rm Contract.docx"}));
    assert_eq!(
        guard_with(&args(&[], &file, &id), hook.to_string().as_bytes()),
        deny("Contract.docx")
    );
}

#[test]
fn a_refusal_adds_one_record_line_naming_the_file() {
    let fx = Fixture::new("no-copy-record");
    let (file, id) = list(&fx, LINES);
    let record = PathBuf::from(fx.path("guard-blocks.jsonl"));
    let record_id = write::create_record(&record).unwrap().hex();
    let hook = fx.hook("apply_patch", &patch("*** Delete File: Free.docx"));
    let before = [
        "--record",
        record.to_str().unwrap(),
        "--record-id",
        &record_id,
    ];
    let codex = args(&before, &file, &id);
    assert_eq!(
        guard_with(&codex, hook.to_string().as_bytes()),
        refused("Free.docx", CLOUD)
    );
    let text = std::fs::read_to_string(&record).unwrap();
    let lines: Vec<Record> = text
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert_eq!(lines[0].reason, Reason::NoCopy);
    assert_eq!(
        lines[0].path.as_ref().map(MatterPath::as_str),
        Some("Free.docx")
    );
}

/// With `file` and `id` as the list, nothing the list says is trusted, and the gate stays shut
/// on what the guard can see: Big.bin, over the cap, is still refused; and the matter's files
/// are untouched.
fn untrusted_but_shut(fx: &Fixture, file: &Path, id: &str) {
    big(fx);
    let codex = args(&[], file, id);
    let rm = |name: &str| {
        fx.hook("Bash", &json!({"command": format!("rm {name}")}))
            .to_string()
    };
    assert_eq!(
        guard_with(&codex, rm("Free.docx").as_bytes()),
        "",
        "the list was trusted"
    );
    assert_eq!(
        guard_with(&codex, rm("Big.bin").as_bytes()),
        refused("Big.bin", LARGE)
    );
    assert_eq!(std::fs::read(fx.path("m/Free.docx")).unwrap(), b"x");
}

#[test]
fn the_list_the_host_made_is_trusted() {
    let fx = Fixture::new("no-copy-trusted");
    let (file, id) = list(&fx, LINES);
    let hook = fx.hook("Bash", &json!({"command": "rm Free.docx"}));
    assert_eq!(
        guard_with(&args(&[], &file, &id), hook.to_string().as_bytes()),
        refused("Free.docx", CLOUD)
    );
}

#[test]
fn a_hard_link_at_the_lists_place_never_opens_the_gate() {
    let fx = Fixture::new("no-copy-hard-link");
    let target = PathBuf::from(fx.path("planted.txt"));
    write::write_file(&target, format!("{HEADER}{LINES}").as_bytes()).unwrap();
    let file = PathBuf::from(fx.path("no-copy.txt"));
    write::hard_link(&target, &file).unwrap();
    // Its identity is the file's own: only the second link makes it untrusted.
    untrusted_but_shut(&fx, &file, &id_of(&target));
}

#[cfg(unix)]
#[test]
fn a_symlink_at_the_lists_place_never_opens_the_gate() {
    let fx = Fixture::new("no-copy-symlink");
    let target = PathBuf::from(fx.path("planted.txt"));
    write::write_file(&target, format!("{HEADER}{LINES}").as_bytes()).unwrap();
    let file = PathBuf::from(fx.path("no-copy.txt"));
    write::symlink(&target, &file).unwrap();
    untrusted_but_shut(&fx, &file, &id_of(&target));
}

/// Windows: a file symlink at the list's place. Making one needs Developer Mode or an elevated
/// token; without either the test says so and passes, the hard link test still covering links.
#[cfg(windows)]
#[test]
fn a_symlink_at_the_lists_place_never_opens_the_gate() {
    let fx = Fixture::new("no-copy-symlink");
    let target = PathBuf::from(fx.path("planted.txt"));
    write::write_file(&target, format!("{HEADER}{LINES}").as_bytes()).unwrap();
    let file = PathBuf::from(fx.path("no-copy.txt"));
    if let Err(error) = write::symlink_file(&target, &file) {
        eprintln!("skipped: no right to make a symlink here ({error})");
        return;
    }
    untrusted_but_shut(&fx, &file, &id_of(&target));
}

#[test]
fn another_file_at_the_lists_place_never_opens_the_gate() {
    let fx = Fixture::new("no-copy-replaced");
    let (file, id) = list(&fx, LINES);
    // The same text in a new file: the identity the host pinned is gone.
    write::remove_file(&file).unwrap();
    write::write_file(&file, format!("{HEADER}{LINES}").as_bytes()).unwrap();
    untrusted_but_shut(&fx, &file, &id);
    untrusted_but_shut(&fx, &file, "not-an-identity");
}

#[test]
fn a_missing_list_or_none_at_all_never_opens_the_gate() {
    let fx = Fixture::new("no-copy-missing");
    let missing = PathBuf::from(fx.path("no-copy.txt"));
    untrusted_but_shut(&fx, &missing, "1-2");
    let folder = PathBuf::from(fx.path("out"));
    untrusted_but_shut(&fx, &folder, "1-2");
    // No list argument at all, or a relative one, or no identity.
    let (file, _) = list(&fx, LINES);
    let rm = fx
        .hook("Bash", &json!({"command": "rm Big.bin"}))
        .to_string();
    let path = file.to_str().unwrap();
    for args in [
        vec![],
        vec!["--no-copy-list", "no-copy.txt", "--no-copy-list-id", "1-2"],
        vec!["--no-copy-list", path],
    ] {
        assert_eq!(
            guard_with(&args, rm.as_bytes()),
            refused("Big.bin", LARGE),
            "{args:?}"
        );
    }
}
