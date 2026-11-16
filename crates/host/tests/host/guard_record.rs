//! The pre-write guard's record of its refusals as a launch makes it: a Codex session from a data
//! folder whose path has a space and a non-ASCII name gets its record in a folder of its own,
//! `<data>/browser/guard/<session>`, with the file's identity after `--record-id` in both hook
//! commands, and passes the trust check; the folder goes with the session. A Claude session, whose
//! hook passes no record, makes neither the folder nor the file. The real hook command run
//! through `cmd /c` with the built app is `crates/desktop/tests/guard_record_windows.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::StopReason;
use serde_json::Value;

use crate::codex::{CODEX, Recording, codex_dir, one_turn, var};
use crate::support::{Dir, Scenario};

/// The guard program the desktop app sets: named in the hook command only, never run here.
// unix-paths-ok: a recording launcher starts nothing, and the path is only text in the command
const PROGRAM: &str = "/opt/clauseo/clauseo-desktop";

/// The parts of `record`'s path below `dir`'s data folder, as the test named it or as the
/// file system names it (`dunce::canonicalize`), whichever `record` starts with. Anchored on
/// the data folder, not on a variable of the agent's (Windows sets no `TMPDIR`).
pub(crate) fn below_data(dir: &Dir, record: &Path) -> Vec<String> {
    let canonical = dunce::canonicalize(dir.data_dir()).unwrap();
    let below = record
        .strip_prefix(dir.data_dir())
        .or_else(|_| record.strip_prefix(&canonical))
        .unwrap_or_else(|_| panic!("{record:?} is not under {:?}", dir.data_dir()));
    below
        .components()
        .map(|part| part.as_os_str().to_str().unwrap().to_owned())
        .collect()
}

/// Every file under `dir`, through its folders; none when it is not there.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .flat_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                files_under(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}

#[test]
fn codex_from_a_spaced_non_ascii_data_folder_gets_its_record_and_passes_the_trust_check() {
    let dir = codex_dir("codex_guard Zoë Q", "codex-hook-trusted");
    let launcher = Recording::new(&dir);
    let envs = Arc::clone(&launcher.envs);
    let host = dir.start_launching(0, Arc::new(launcher), |config| {
        config.with_pre_write_guard(PathBuf::from(PROGRAM))
    });
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), "Guarded.", "the trust check passed");

    let config = {
        let envs = envs.lock().unwrap();
        let [codex] = envs.as_slice() else {
            panic!("one launch, not {}", envs.len());
        };
        let config: Value = serde_json::from_str(&var(codex, "CODEX_CONFIG").unwrap()).unwrap();
        config
    };
    let hook = &config["hooks"]["PreToolUse"][0]["hooks"][0];
    let windows = hook["commandWindows"].as_str().unwrap();
    let (record, id) = windows
        .strip_prefix(&format!("\"{PROGRAM}\" --pre-write-guard --record \""))
        .and_then(|rest| rest.split_once("\" --record-id "))
        .unwrap_or_else(|| panic!("{windows}"));
    let record = PathBuf::from(record);
    let posix = format!(
        "'{PROGRAM}' --pre-write-guard --record '{}' --record-id {id}",
        record.display()
    );
    assert_eq!(hook["command"], posix);
    assert!(dir.data_dir().to_str().unwrap().contains("Zoë Q"));
    // The one session's guard folder, `<data>/browser/guard/<session>`, holds the record.
    let guards = dir.data_dir().join("browser").join("guard");
    let sessions: Vec<PathBuf> = std::fs::read_dir(&guards)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    let [folder] = sessions.as_slice() else {
        panic!("one session folder: {sessions:?}");
    };
    let session = folder.file_name().unwrap().to_str().unwrap();
    assert_eq!(
        below_data(&dir, &record),
        ["browser", "guard", session, "guard-blocks.jsonl"]
    );
    assert_eq!(
        dunce::canonicalize(&record).unwrap(),
        dunce::canonicalize(folder.join("guard-blocks.jsonl")).unwrap()
    );
    let folder = folder.clone();
    assert_eq!(
        std::fs::read(&record).unwrap(),
        b"",
        "made empty, nothing refused"
    );
    assert_eq!(id.len(), 48);
    assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()), "{id}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let meta = std::fs::metadata(&record).unwrap();
        assert_eq!(
            id,
            format!("{:016x}{:032x}", meta.dev(), u128::from(meta.ino()))
        );
    }
    let expected = clauseo_host::codex_hook_config(Path::new(PROGRAM), Some((&record, id)));
    assert_eq!(config["hooks"], expected.unwrap());
    host.shutdown();
    assert!(!folder.exists(), "the guard folder goes with the session");
}

/// Claude's hook passes no record: its session makes no guard folder and no record file
/// anywhere under the data folder, while it is live. (The launch makes the record whether or not
/// a guard program is set, so none is set here.)
#[test]
fn a_claude_session_makes_no_guard_folder_or_record() {
    let dir = Dir::new(
        "guard_record_claude",
        &[("claude-acp", Scenario::Bundled("claude-prompt-stream"))],
    );
    let host = dir.start(0);
    let (_, events, turn) = one_turn(&dir, &host, "claude-acp");
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert!(!dir.data_dir().join("browser").join("guard").exists());
    let records: Vec<PathBuf> = files_under(dir.data_dir())
        .into_iter()
        .filter(|file| file.ends_with("guard-blocks.jsonl"))
        .collect();
    assert_eq!(records, Vec::<PathBuf>::new());
    host.shutdown();
}
