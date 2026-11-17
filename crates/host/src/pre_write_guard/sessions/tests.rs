//! The guard's session check (Codex's hook with `--sessions`): the conversation's sessions and
//! their children pass to the guard's other checks; any other session, and every call the guard
//! can't place, is refused with exit 2 and the reason on stderr. The process-wide part
//! ([`super::arm_with`]: a panic, the deadline) runs in a child process, the test binary run again.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::super::{DENY, record, run_timed};
use super::{DATA_IMAGE, FAILED, FLAG, OWN_FLAG, REFUSED, TOO_LATE, add, data_of, file, read};
use crate::write;

/// The conversation's session id, as Codex's thread ids look.
const MAIN: &str = "01a1e4b4-30ac-7f81-b3fe-c7d36d7f97b5";

/// A data folder with a session's guard folder holding the ids file with [`MAIN`], a matter
/// folder `m` beside it, and a file in the data folder. Removed on drop.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(test: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "clauseo-host-sessions-{test}-{}",
            std::process::id()
        ));
        write::remove_dir_all(&root).unwrap();
        write::create_dir_all(&root.join("m")).unwrap();
        let fixture = Self { root };
        write::create_dir_all(&fixture.folder()).unwrap();
        write::write_file(&fixture.data().join("store.png"), b"x").unwrap();
        write::write_file(&fixture.root.join("m").join("own.png"), b"x").unwrap();
        add(&fixture.folder(), MAIN).unwrap();
        fixture
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn folder(&self) -> PathBuf {
        record::folder(&self.data(), "s1")
    }

    fn ids(&self) -> PathBuf {
        file(&self.folder())
    }

    fn args(&self) -> Vec<OsString> {
        vec![OsString::from(FLAG), self.ids().into_os_string()]
    }

    /// The hook input for `tool` with `input` from the session `session`, Codex's shape.
    fn hook(&self, session: Option<&str>, tool: &str, input: &Value) -> Value {
        let mut hook = json!({
            "turn_id": "t-1",
            "transcript_path": "/nowhere/t.jsonl",
            "cwd": self.root.join("m").to_str().unwrap(),
            "hook_event_name": "PreToolUse",
            "model": "gpt-5.6",
            "permission_mode": "default",
            "tool_name": tool,
            "tool_input": input,
            "tool_use_id": "call_1",
        });
        if let Some(session) = session {
            hook["session_id"] = Value::from(session);
        }
        hook
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.root));
    }
}

/// The guard on `input` with `args`: its exit code and stderr.
fn guard(args: &[OsString], input: &[u8]) -> (i32, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_timed(args, input, (&mut out, &mut err), false);
    assert!(out.is_empty(), "stdout stays empty");
    (code, String::from_utf8(err).unwrap())
}

fn bash(command: &str) -> Value {
    json!({ "command": command })
}

/// Refused as Codex blocks: exit 2 and [`REFUSED`] on stderr.
fn assert_refused((code, err): (i32, String)) {
    assert_eq!(code, DENY, "{err}");
    assert_eq!(err.trim_end(), REFUSED);
}

#[test]
fn the_conversations_session_passes_and_the_guards_checks_still_apply() {
    let fixture = Fixture::new("main");
    let hook = fixture.hook(Some(MAIN), "Bash", &bash("ls"));
    assert_eq!(
        guard(&fixture.args(), hook.to_string().as_bytes()),
        (0, String::new())
    );
    let patch = "*** Begin Patch\n*** Add File: .codex/config.toml\n+x\n*** End Patch\n";
    let hook = fixture.hook(Some(MAIN), "apply_patch", &json!({ "command": patch }));
    let (code, err) = guard(&fixture.args(), hook.to_string().as_bytes());
    assert_eq!(code, DENY);
    assert!(err.contains(".codex folder"), "{err}");
}

/// A `multi_agent` child carries the conversation's `session_id` with its own `agent_id`.
#[test]
fn a_child_of_the_conversations_session_passes() {
    let fixture = Fixture::new("child");
    let mut hook = fixture.hook(Some(MAIN), "Bash", &bash("ls"));
    hook["agent_id"] = Value::from("01a1e4b4-3a00-7000-8000-000000000001");
    hook["agent_type"] = Value::from("worker");
    assert_eq!(
        guard(&fixture.args(), hook.to_string().as_bytes()),
        (0, String::new())
    );
}

/// Every tool reaches the guard (matcher `*`); one with no path to check passes for the
/// conversation's session and is refused for any other.
#[test]
fn a_tool_with_no_path_passes_only_for_the_conversations_session() {
    let fixture = Fixture::new("tools");
    for (tool, input) in [
        ("spawn_agent", json!({ "message": "x" })),
        (
            "mcp__browser__navigate",
            json!({ "url": "https://example.com" }),
        ),
        ("update_plan", json!({ "plan": [] })),
    ] {
        let hook = fixture.hook(Some(MAIN), tool, &input);
        assert_eq!(
            guard(&fixture.args(), hook.to_string().as_bytes()),
            (0, String::new()),
            "{tool}"
        );
        let hook = fixture.hook(Some("01a1e4b4-3233-75d0-93de-ab9057966fe4"), tool, &input);
        assert_refused(guard(&fixture.args(), hook.to_string().as_bytes()));
    }
}

/// A thread Codex starts on its own: its own fresh id and no transcript.
#[test]
fn a_fresh_session_with_no_transcript_is_refused() {
    let fixture = Fixture::new("fresh");
    let mut hook = fixture.hook(
        Some("01a1e4b4-3233-75d0-93de-ab9057966fe4"),
        "Bash",
        &bash("cat /etc/hosts"),
    );
    hook["transcript_path"] = Value::Null;
    assert_refused(guard(&fixture.args(), hook.to_string().as_bytes()));
}

#[test]
fn a_made_up_session_is_refused() {
    let fixture = Fixture::new("made-up");
    for session in [
        "s-1",
        "01A1E4B4-30AC-7F81-B3FE-C7D36D7F97B5",
        " ",
        MAIN.get(1..).unwrap(),
    ] {
        let hook = fixture.hook(Some(session), "Bash", &bash("ls"));
        assert_refused(guard(&fixture.args(), hook.to_string().as_bytes()));
    }
}

#[test]
fn a_missing_or_empty_session_id_is_refused() {
    let fixture = Fixture::new("no-id");
    let hook = fixture.hook(None, "Bash", &bash("ls"));
    assert_refused(guard(&fixture.args(), hook.to_string().as_bytes()));
    for id in [Value::from(""), Value::from(7), Value::Null, json!([MAIN])] {
        let mut hook = fixture.hook(None, "Bash", &bash("ls"));
        hook["session_id"] = id;
        assert_refused(guard(&fixture.args(), hook.to_string().as_bytes()));
    }
}

/// No ids file, a relative one, or the flag with no file after it.
#[test]
fn a_missing_ids_file_is_refused() {
    let fixture = Fixture::new("no-file");
    let hook = fixture.hook(Some(MAIN), "Bash", &bash("ls")).to_string();
    write::remove_file(&fixture.ids()).unwrap();
    assert_refused(guard(&fixture.args(), hook.as_bytes()));
    add(&fixture.folder(), MAIN).unwrap();
    for args in [
        vec![OsString::from(FLAG)],
        vec![OsString::from(FLAG), OsString::from("sessions.json")],
    ] {
        assert_refused(guard(&args, hook.as_bytes()));
    }
}

#[test]
fn an_unparsable_ids_file_is_refused() {
    let fixture = Fixture::new("bad-file");
    let hook = fixture.hook(Some(MAIN), "Bash", &bash("ls")).to_string();
    let big = format!("[\"{MAIN}\", \"{}\"]", "a".repeat(70 * 1024));
    for text in [
        "not json".to_owned(),
        format!("[\"{MAIN}\""),
        format!("{{\"ids\": [\"{MAIN}\"]}}"),
        format!("\"{MAIN}\""),
        format!("[\"{MAIN}\", \"\"]"),
        format!("[\"{MAIN}\", 7]"),
        big,
    ] {
        write::write_file(&fixture.ids(), text.as_bytes()).unwrap();
        assert_refused(guard(&fixture.args(), hook.as_bytes()));
    }
    // A folder at the file's place cannot be read.
    write::remove_file(&fixture.ids()).unwrap();
    write::create_dir_all(&fixture.ids()).unwrap();
    assert_refused(guard(&fixture.args(), hook.as_bytes()));
}

/// Under the session check, stdin the guard cannot read refuses, where without it the guard
/// lets the tool go (`IGNORED`).
#[test]
fn malformed_stdin_is_refused() {
    let fixture = Fixture::new("stdin");
    let cut = fixture.hook(Some(MAIN), "Bash", &bash("ls")).to_string();
    let cut = cut.get(..cut.len() / 2).unwrap().to_owned();
    for input in ["", "   ", "{", "not json", "[]", "null", cut.as_str()] {
        assert_refused(guard(&fixture.args(), input.as_bytes()));
    }
    let mut invalid = br#"{"session_id": ""#.to_vec();
    invalid.extend([0xff, 0xfe]);
    assert_refused(guard(&fixture.args(), &invalid));
}

/// A reader that fails: stdin that cannot be read refuses.
#[test]
fn unreadable_stdin_is_refused() {
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken pipe"))
        }
    }
    let fixture = Fixture::new("unreadable");
    let mut err = Vec::new();
    let code = run_timed(&fixture.args(), Broken, (Vec::new(), &mut err), false);
    assert_eq!(code, DENY);
    assert_eq!(String::from_utf8(err).unwrap().trim_end(), REFUSED);
}

/// The call the launch's self-test sends is refused by the guard itself.
#[test]
fn the_self_tests_call_is_refused() {
    let fixture = Fixture::new("self-test");
    let input = super::super::self_test::input(&fixture.root.join("m"));
    assert_refused(guard(&fixture.args(), &input));
}

/// `view_image` of a file in the data folder is refused for the conversation's own session,
/// by its path, relative from `cwd`, and through a link; one in the matter passes.
#[test]
fn view_image_of_a_data_folder_file_is_refused() {
    let fixture = Fixture::new("view-image");
    let image = |path: &str| fixture.hook(Some(MAIN), "view_image", &json!({ "path": path }));
    let data_file = fixture.data().join("store.png");
    let refused = |hook: Value| {
        let (code, err) = guard(&fixture.args(), hook.to_string().as_bytes());
        assert_eq!(code, DENY, "{hook}");
        assert_eq!(err.trim_end(), DATA_IMAGE);
    };
    refused(image(data_file.to_str().unwrap()));
    refused(image("../data/store.png"));
    refused(fixture.hook(Some(MAIN), "view_image", &json!({})));
    #[cfg(unix)]
    {
        write::symlink(&fixture.data(), &fixture.root.join("m").join("link")).unwrap();
        refused(image("link/store.png"));
    }
    for path in [
        "own.png",
        fixture.root.join("m").join("own.png").to_str().unwrap(),
    ] {
        assert_eq!(
            guard(&fixture.args(), image(path).to_string().as_bytes()),
            (0, String::new()),
            "{path}"
        );
    }
}

/// The session's run folder and its matter's notes in the data folder, as the host lays them out
/// (`actor/turn/launch_checks.rs`, `RunFolders`), made, with an image in the run's work folder;
/// and the hook's arguments naming them as the session's own.
fn own_folders(fixture: &Fixture) -> (PathBuf, Vec<OsString>) {
    let run = fixture.data().join("run").join("s1");
    let notes = fixture.data().join("agent-state").join("m1").join("notes");
    write::create_dir_all(&run.join("work")).unwrap();
    write::create_dir_all(&notes).unwrap();
    write::write_file(&run.join("work").join("page-1.png"), b"x").unwrap();
    write::write_file(&notes.join("chart.png"), b"x").unwrap();
    let mut args = fixture.args();
    for folder in [&run, &notes] {
        args.extend([OsString::from(OWN_FLAG), folder.clone().into_os_string()]);
    }
    (run, args)
}

/// A `view_image` of a page rendered into the session's work folder, or of an image in its
/// matter's notes, is allowed when the hook names them as its own.
#[test]
fn view_image_in_the_sessions_own_folders_is_allowed() {
    let fixture = Fixture::new("view-own");
    let (run, args) = own_folders(&fixture);
    let notes = fixture.data().join("agent-state").join("m1").join("notes");
    for path in [run.join("work").join("page-1.png"), notes.join("chart.png")] {
        let hook = fixture.hook(
            Some(MAIN),
            "view_image",
            &json!({ "path": path.to_str().unwrap() }),
        );
        assert_eq!(
            guard(&args, hook.to_string().as_bytes()),
            (0, String::new()),
            "{path:?}"
        );
        // Without the own folders named, it is refused as before.
        let (code, err) = guard(&fixture.args(), hook.to_string().as_bytes());
        assert_eq!((code, err.trim_end()), (DENY, DATA_IMAGE), "{path:?}");
    }
}

/// Another run's work folder, and the rest of the data folder, stay refused for a `view_image`.
#[test]
fn view_image_in_another_runs_work_folder_is_refused() {
    let fixture = Fixture::new("view-other");
    let (_, args) = own_folders(&fixture);
    let other = fixture.data().join("run").join("s2").join("work");
    write::create_dir_all(&other).unwrap();
    write::write_file(&other.join("page-1.png"), b"x").unwrap();
    for path in [other.join("page-1.png"), fixture.data().join("store.png")] {
        let hook = fixture.hook(
            Some(MAIN),
            "view_image",
            &json!({ "path": path.to_str().unwrap() }),
        );
        let (code, err) = guard(&args, hook.to_string().as_bytes());
        assert_eq!((code, err.trim_end()), (DENY, DATA_IMAGE), "{path:?}");
    }
}

/// A link in the session's work folder to elsewhere in the data folder is judged by where it
/// leads: refused.
#[cfg(unix)]
#[test]
fn view_image_through_a_link_in_the_work_folder_is_refused() {
    let fixture = Fixture::new("view-link");
    let (run, args) = own_folders(&fixture);
    let other = fixture.data().join("run").join("s2").join("work");
    write::create_dir_all(&other).unwrap();
    write::write_file(&other.join("page-1.png"), b"x").unwrap();
    let work = run.join("work");
    write::symlink(&fixture.data().join("store.png"), &work.join("store.png")).unwrap();
    write::symlink(&other, &work.join("other")).unwrap();
    for path in [
        work.join("store.png"),
        work.join("other").join("page-1.png"),
    ] {
        let hook = fixture.hook(
            Some(MAIN),
            "view_image",
            &json!({ "path": path.to_str().unwrap() }),
        );
        let (code, err) = guard(&args, hook.to_string().as_bytes());
        assert_eq!((code, err.trim_end()), (DENY, DATA_IMAGE), "{path:?}");
    }
}

/// Recorded from Codex 0.162.1 under codex-acp 2.2.2 (`tests/fixtures/codex/README.md`): the
/// hook's `session_id` for the session's own call, and its `multi_agent` child's, is the
/// `sessionId` `session/new` returned, which the host lists; another thread's is not.
#[test]
fn codexs_hook_session_id_is_the_acp_session_id() {
    let recorded: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/codex/hook-session-id.json"
    ))
    .unwrap();
    let acp = recorded["session_new"]["sessionId"].as_str().unwrap();
    let hooks = &recorded["hooks"];
    for name in ["session", "child"] {
        assert_eq!(hooks[name]["session_id"], acp, "{name}");
    }
    assert_ne!(hooks["other_thread"]["session_id"], acp);
    let fixture = Fixture::new("recorded");
    write::remove_file(&fixture.ids()).unwrap();
    add(&fixture.folder(), acp).unwrap();
    let call = |name: &str| {
        // The recorded call, in this test's matter folder.
        let mut hook = hooks[name].clone();
        hook["cwd"] = Value::from(fixture.root.join("m").to_str().unwrap());
        guard(&fixture.args(), hook.to_string().as_bytes())
    };
    assert_eq!(call("session"), (0, String::new()));
    assert_eq!(call("child"), (0, String::new()));
    assert_refused(call("other_thread"));
}

/// A fork's new id joins the ids already there; an id already there writes nothing new.
#[test]
fn adding_a_session_keeps_the_ones_there() {
    let fixture = Fixture::new("add");
    let fork = "01a1e4b4-4000-7000-8000-00000000f0f0";
    add(&fixture.folder(), fork).unwrap();
    add(&fixture.folder(), MAIN).unwrap();
    assert_eq!(read(&fixture.ids()).unwrap(), [MAIN, fork]);
    let hook = fixture.hook(Some(fork), "Bash", &bash("ls"));
    assert_eq!(
        guard(&fixture.args(), hook.to_string().as_bytes()),
        (0, String::new())
    );
    assert!(add(&fixture.folder(), "").is_err());
}

#[test]
fn the_data_folder_is_found_from_the_ids_file() {
    let data = Path::new("/d/Clauseo data");
    let ids = file(&record::folder(data, "s1"));
    assert_eq!(data_of(&ids), Some(data));
}

/// The environment variable that makes a test below act as the guard's process.
const CHILD: &str = "CLAUSEO_TEST_GUARD_SESSIONS_CHILD";

/// Runs this test binary's test `test` alone, with [`CHILD`] set: its exit code, its stderr and
/// how long it took.
#[expect(
    clippy::disallowed_methods,
    reason = "the guard's process is this test binary run again, timed in real time"
)]
fn child(test: &str) -> (Option<i32>, String, Duration) {
    let start = Instant::now();
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("pre_write_guard::sessions::tests::{test}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        start.elapsed(),
    )
}

/// A panic inside the guard's process refuses: exit 2 and [`FAILED`] on stderr.
#[test]
fn a_panic_in_the_guard_exits_2_with_a_line() {
    if std::env::var_os(CHILD).is_some() {
        super::arm_with(Duration::from_secs(60));
        panic!("a panic inside the guard");
    }
    let (code, err, _) = child("a_panic_in_the_guard_exits_2_with_a_line");
    assert_eq!(code, Some(DENY), "{err}");
    assert!(err.lines().any(|line| line == FAILED), "{err}");
}

/// A guard that does not finish in time refuses on its own at its deadline, with [`TOO_LATE`].
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the guard's process outlives its deadline"
)]
fn a_guard_past_its_deadline_exits_2_with_a_line() {
    if std::env::var_os(CHILD).is_some() {
        super::arm_with(Duration::from_millis(300));
        std::thread::sleep(Duration::from_secs(60));
        return;
    }
    let (code, err, took) = child("a_guard_past_its_deadline_exits_2_with_a_line");
    assert_eq!(code, Some(DENY), "{err}");
    assert!(err.lines().any(|line| line == TOO_LATE), "{err}");
    assert!(took < Duration::from_secs(30), "{took:?}");
}

/// Claude Code's entry has no `--sessions`: nothing is armed and no session is checked.
#[test]
fn without_the_flag_nothing_changes() {
    assert!(!super::wanted(&[
        OsString::from("--matter"),
        OsString::from("/m")
    ]));
    let fixture = Fixture::new("no-flag");
    let hook = fixture.hook(Some("other"), "Bash", &bash("ls"));
    assert_eq!(guard(&[], hook.to_string().as_bytes()), (0, String::new()));
}

/// Each decision under the session check goes to the session's record file as one line the host
/// logs (session, tool, decision, time), which no refusal record parses as.
#[test]
fn each_decision_is_recorded_for_the_log() {
    let fixture = Fixture::new("decisions");
    write::remove_dir_all(&fixture.folder()).unwrap();
    let (record, id) = crate::pre_write_guard::make_record(&fixture.data(), "s1").unwrap();
    add(&fixture.folder(), MAIN).unwrap();
    let mut args = vec![
        OsString::from(record::FLAG),
        record.clone().into_os_string(),
        OsString::from(record::ID_FLAG),
        OsString::from(id),
    ];
    args.extend(fixture.args());
    let main = fixture.hook(Some(MAIN), "view_image", &json!({ "path": "own.png" }));
    assert_eq!(guard(&args, main.to_string().as_bytes()).0, 0);
    let other = fixture.hook(Some("other"), "Bash", &bash("ls"));
    assert_refused(guard(&args, other.to_string().as_bytes()));
    let text = std::fs::read_to_string(&record).unwrap();
    let decisions: Vec<super::Decision> = text
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let seen: Vec<(&str, &str, &str)> = decisions
        .iter()
        .map(|d| (d.session.as_str(), d.tool.as_str(), d.verdict.as_str()))
        .collect();
    assert_eq!(
        seen,
        [
            (MAIN, "view_image", "allowed"),
            ("other", "Bash", "refused")
        ]
    );
    assert!(
        decisions
            .iter()
            .all(|d| d.guard == "decision" && d.ms < 10_000)
    );
    for line in text.lines().filter(|line| !line.is_empty()) {
        assert!(
            serde_json::from_str::<record::Record>(line).is_err(),
            "{line}"
        );
    }
    super::log_decisions(text.lines().map(str::as_bytes));
}
