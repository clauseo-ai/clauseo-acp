//! The fence file on a real disk, and the guard's fail-closed path: a changed or missing file,
//! input it can't read, a panic and the deadline all refuse; the budget and the cost per call.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::json;

use super::decide::Denial;
use super::guard::{DEADLINE, Watch, check, guarded};
use super::{FILE_NAME, FenceFile, RootKind, Untrusted, Written, load};
use crate::agent::data_fence::LaunchDeny;
use crate::clock::{Clock, SystemClock};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// A data folder with a store, a matter with a brief, another matter, and the fence the host
/// writes for them in a run folder: the launch's data rules and the other matter whole.
struct Setup {
    _dir: TempDir,
    data: PathBuf,
    matter: PathBuf,
    other: PathBuf,
    written: Written,
}

fn setup(test: &str) -> Setup {
    let dir = TempDir::new(test);
    let data = dir.path().join("Clauseo Data");
    let matter = dir.path().join("Matters").join("Smith");
    let other = dir.path().join("Matters").join("Jones");
    let run = data.join("run").join("r1");
    for folder in [&run, &matter, &other, &data.join("tools")] {
        write::create_dir_all(folder).unwrap();
    }
    write::write_file(&data.join("state.sqlite"), b"store").unwrap();
    write::write_file(&matter.join("brief.docx"), b"brief").unwrap();
    write::write_file(&other.join("lease.pdf"), b"lease").unwrap();
    let deny = LaunchDeny::fixed(vec![data.clone()]);
    let fence = FenceFile::launch(&deny).with_roots(RootKind::Matter, [&other]);
    let written = fence.write_in(&run).unwrap();
    Setup {
        _dir: dir,
        data,
        matter,
        other,
        written,
    }
}

impl Setup {
    /// The guard's arguments with the fence.
    fn args(&self) -> Vec<OsString> {
        let digest = self.written.digest.clone();
        self.args_with(&self.written.path, &digest)
    }

    fn args_with(&self, file: &Path, digest: &str) -> Vec<OsString> {
        vec![
            "--matter".into(),
            self.matter.clone().into(),
            "--fence".into(),
            file.into(),
            digest.into(),
        ]
    }

    /// The fence's answer to `tool` with `input`, from the matter.
    fn check(&self, tool: &str, input: &serde_json::Value) -> Option<Denial> {
        let hook = json!({"tool_name": tool, "tool_input": input, "cwd": self.matter});
        check(
            &self.args(),
            hook.to_string().as_bytes(),
            Some(&self.matter),
        )
    }
}

fn file(path: &Path) -> serde_json::Value {
    json!({"file_path": path})
}

#[test]
fn the_fence_file_names_the_launch_rules_and_a_changed_one_is_not_trusted() {
    let setup = setup("fence-file");
    let fence = load(&setup.written.path, &setup.written.digest).unwrap();
    assert_eq!(setup.written.path.file_name().unwrap(), FILE_NAME);
    let [data, other] = fence.roots.as_slice() else {
        panic!("{fence:?}");
    };
    assert_eq!(data.kind, RootKind::Data);
    let real = dunce::canonicalize(&setup.data).unwrap();
    assert_eq!(data.spellings.first().map(PathBuf::from), Some(real));
    assert!(data.read.contains(&"run/*/file-fence.json".to_owned()));
    assert!(data.read.contains(&"state.sqlite*".to_owned()));
    assert!(data.edit.contains(&"tools/**".to_owned()));
    assert!(data.id.is_some() && data.line.len() > 1);
    assert_eq!(
        (other.kind, other.read.as_slice()),
        (RootKind::Matter, &["**".to_owned()][..])
    );
    let upper = setup.written.digest.to_uppercase();
    assert!(load(&setup.written.path, &upper).is_ok());
    let wrong = "0".repeat(64);
    assert_eq!(load(&setup.written.path, &wrong), Err(Untrusted::Changed));
    let missing = setup.written.path.with_file_name("gone.json");
    assert_eq!(
        load(&missing, &setup.written.digest),
        Err(Untrusted::Unreadable)
    );
    let empty = br#"{"version":1,"roots":[]}"#;
    write::replace_private_file(&setup.written.path, empty).unwrap();
    assert_eq!(
        load(&setup.written.path, &setup.written.digest),
        Err(Untrusted::Changed)
    );
    let not_fence = b"{}";
    write::replace_private_file(&setup.written.path, not_fence).unwrap();
    assert_eq!(
        load(&setup.written.path, &super::digest(not_fence)),
        Err(Untrusted::Malformed)
    );
}

#[test]
fn an_invited_matter_is_fenced_to_the_writing_tools_alone() {
    let setup = setup("fence-edit-roots");
    let fence = FenceFile::launch(&LaunchDeny::fixed(vec![setup.data.clone()]))
        .with_edit_roots(RootKind::Matter, [&setup.other]);
    let root = fence.roots.last().unwrap();
    assert_eq!(root.read, Vec::<String>::new());
    assert_eq!(root.edit, vec!["**".to_owned()]);
}

#[test]
fn the_guard_refuses_a_fenced_file_on_the_real_disk() {
    let setup = setup("fence-real");
    let store = setup.data.join("state.sqlite");
    let lease = setup.other.join("lease.pdf");
    assert_eq!(
        setup.check("Read", &file(&store)),
        Some(Denial::Fenced(RootKind::Data))
    );
    assert_eq!(
        setup.check("Edit", &file(&lease)),
        Some(Denial::Fenced(RootKind::Matter))
    );
    let new = setup.other.join("Drafts").join("new.docx");
    assert_eq!(
        setup.check("Write", &file(&new)),
        Some(Denial::Fenced(RootKind::Matter))
    );
    assert_eq!(
        setup.check("Read", &file(&setup.matter.join("brief.docx"))),
        None
    );
    let parent = setup.matter.parent().unwrap();
    let grep = json!({"pattern": "lease", "path": parent});
    assert_eq!(
        setup.check("Grep", &grep),
        Some(Denial::Holds(RootKind::Matter))
    );
    assert_eq!(setup.check("Glob", &json!({"pattern": "**/*.docx"})), None);
    assert_eq!(setup.check("Bash", &json!({"command": "cat x"})), None);
}

#[cfg(unix)]
#[test]
fn the_guard_follows_a_link_on_the_real_disk() {
    let setup = setup("fence-link");
    write::symlink(&setup.data, &setup.matter.join("shortcut")).unwrap();
    let via = setup.matter.join("shortcut").join("state.sqlite");
    assert_eq!(
        setup.check("Read", &file(&via)),
        Some(Denial::Fenced(RootKind::Data))
    );
}

/// macOS's temporary folder is under `/var`, a link to `/private/var`: either spelling lands in
/// the fenced folder.
#[cfg(target_os = "macos")]
#[test]
fn var_and_private_var_land_in_one_place_on_the_real_disk() {
    let setup = setup("fence-var");
    let real = dunce::canonicalize(&setup.data).unwrap();
    let text = real.to_str().unwrap();
    let other = match text.strip_prefix("/private") {
        Some(rest) => PathBuf::from(rest),
        None => PathBuf::from(format!("/private{text}")),
    };
    if other.exists() {
        let store = other.join("state.sqlite");
        assert_eq!(
            setup.check("Read", &file(&store)),
            Some(Denial::Fenced(RootKind::Data))
        );
    }
    let shouted = PathBuf::from(text.to_uppercase()).join("STATE.SQLITE");
    assert_eq!(
        setup.check("Read", &file(&shouted)),
        Some(Denial::Fenced(RootKind::Data))
    );
}

#[test]
fn input_or_a_fence_the_guard_cannot_trust_refuses_every_checked_tool() {
    let setup = setup("fence-broken");
    let read = json!({"tool_name": "Read", "tool_input": {"file_path": "x"}, "cwd": setup.matter});
    let read = read.to_string();
    let broken = Some(Denial::Broken);
    let matter = Some(setup.matter.as_path());
    assert_eq!(check(&setup.args(), b"not json", matter), broken);
    assert_eq!(
        check(&setup.args(), br#"{"tool_input":{}}"#, matter),
        broken
    );
    assert_eq!(
        check(&setup.args(), br#"{"tool_name":"Read"}"#, matter),
        broken
    );
    let wrong = setup.args_with(&setup.written.path, &"0".repeat(64));
    assert_eq!(check(&wrong, read.as_bytes(), matter), broken);
    let gone = setup.args_with(&setup.written.path.with_file_name("gone"), "00");
    assert_eq!(check(&gone, read.as_bytes(), matter), broken);
    let cut: Vec<OsString> = setup.args().into_iter().take(4).collect();
    assert_eq!(check(&cut, read.as_bytes(), matter), broken);
    // No fence argument: the fence does not run (Codex's hook, a launch with no fence file).
    let none: Vec<OsString> = setup.args().into_iter().take(2).collect();
    assert_eq!(check(&none, b"not json", matter), None);
}

#[test]
fn a_panic_in_the_fence_refuses() {
    assert_eq!(guarded(|| panic!("the fence failed")), Some(Denial::Broken));
    assert_eq!(guarded(|| None), None);
}

#[test]
fn the_deadline_refuses_unless_the_fence_decided_first() {
    let (sent, fired) = mpsc::channel();
    let _watch = Watch::arm(Duration::from_millis(20), move || sent.send(()).unwrap());
    assert!(
        fired.recv_timeout(Duration::from_secs(10)).is_ok(),
        "the deadline refuses"
    );
    let (sent, fired) = mpsc::channel();
    let watch = Watch::arm(Duration::from_millis(300), move || sent.send(()).unwrap());
    watch.disarm();
    assert!(
        fired.recv_timeout(Duration::from_secs(1)).is_err(),
        "a decided fence stays quiet"
    );
}

/// The guard's whole budget, process start included, is Claude Code's 10 seconds; the fence's
/// deadline leaves most of it.
#[test]
fn the_fence_deadline_is_far_under_claude_codes_hook_timeout() {
    let timeout = Duration::from_secs(crate::agent::loads::GUARD_TIMEOUT_SECONDS);
    assert!(
        DEADLINE.saturating_mul(4) <= timeout,
        "{DEADLINE:?} against {timeout:?}"
    );
    assert!(
        DEADLINE >= Duration::from_millis(500),
        "room for a cold disk"
    );
}

#[test]
fn the_guard_entry_writes_the_refusal_and_its_code() {
    let setup = setup("fence-entry");
    let hook = json!({
        "tool_name": "Read",
        "tool_input": {"file_path": setup.data.join("state.sqlite")},
        "cwd": setup.matter,
    });
    let mut errors = Vec::new();
    let input = hook.to_string();
    let code = crate::pre_write_guard::run_timed(
        &setup.args(),
        input.as_bytes(),
        (Vec::new(), &mut errors),
        false,
    );
    assert_eq!(code, crate::pre_write_guard::DENY);
    let errors = String::from_utf8(errors).unwrap();
    assert!(
        errors.contains("has been denied by your permission settings"),
        "{errors}"
    );
    assert!(!errors.contains("state.sqlite"), "no path: {errors}");
}

/// The cost of one decision (resolve and decide, the fence file read and checked), on this
/// machine; printed for the record, and bounded far under the deadline.
#[test]
fn the_fence_is_fast() {
    let setup = setup("fence-cost");
    let mut deep = setup.matter.clone();
    for name in [
        "Correspondence",
        "2026",
        "November",
        "Counterparty",
        "Drafts",
        "Final",
    ] {
        deep.push(name);
    }
    write::create_dir_all(&deep).unwrap();
    let cases = [
        ("Read", file(&deep.join("letter.docx"))),
        ("Read", file(&setup.data.join("state.sqlite"))),
        ("Grep", json!({"pattern": "x", "path": deep})),
    ];
    for (tool, input) in cases {
        let rounds = 200_u32;
        let start = SystemClock.now().mono;
        let mut slowest = Duration::ZERO;
        for _ in 0..rounds {
            let one = SystemClock.now().mono;
            let _decided = setup.check(tool, &input);
            slowest = slowest.max(SystemClock.now().mono.saturating_sub(one));
        }
        let mean = SystemClock.now().mono.saturating_sub(start) / rounds;
        eprintln!("fence cost {tool}: mean {mean:?}, slowest {slowest:?} over {rounds} calls");
        assert!(mean < DEADLINE / 20, "{tool}: {mean:?}");
    }
}

#[test]
fn a_pattern_covers_what_is_under_what_it_matches() {
    use super::pattern::Pattern;
    let names = |text: &str| -> Vec<String> {
        text.split('/')
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let token = Pattern::new("run/*/relay-token", false);
    assert!(token.covers(&names("run/r1/relay-token")));
    assert!(token.covers(&names("run/r1/relay-token/x")));
    assert!(!token.covers(&names("run/r1/work/relay-token")));
    assert!(!token.covers(&names("run/r1")));
    assert!(token.may_hold(&names("run/r1")));
    assert!(token.may_hold(&names("")));
    assert!(!token.may_hold(&names("run/r1/work")));
    let deep = Pattern::new("agents/claude-acp/home/projects/*/*/subagents/**", false);
    assert!(deep.covers(&names(
        "agents/claude-acp/home/projects/k/s/subagents/a.jsonl"
    )));
    assert!(deep.may_hold(&names("agents/claude-acp/home/projects/k")));
    let all = Pattern::new("**", false);
    assert!(all.covers(&names("")) && all.covers(&names("a/b")));
    let store = Pattern::new("state.sqlite*", true);
    assert!(store.covers(&names("state.sqlite-wal")));
    assert!(store.covers(&[crate::network_paths::fold_name("STATE.SQLITE")]));
    let escaped = Pattern::new("odd\\*name/**", false);
    assert!(escaped.covers(&names("odd*name/x")));
    assert!(!escaped.covers(&names("oddXname/x")));
}

#[test]
fn a_glob_starts_at_its_last_plain_folder() {
    use super::pattern::glob_base;
    let base = |pattern: &str, windows| glob_base(pattern, windows).map(|(base, _)| base);
    // unix-paths-ok: pure text, it never reaches a path API
    assert_eq!(base("/home/a/**/*.md", false), Some("/home/a".to_owned()));
    assert_eq!(base("**/*.md", false), Some(String::new()));
    assert_eq!(base("src/lib.rs", false), Some("src/lib.rs".to_owned()));
    assert_eq!(
        base("C:\\Users\\a\\*.pdf", true),
        Some("C:\\Users\\a".to_owned())
    );
    assert_eq!(base("x/*/../y", false), None);
    assert_eq!(base("{a,b}/*.md", false), Some(String::new()));
    assert_eq!(base("{/etc,/home}/*", false), None);
    assert_eq!(base("x/*:/y", true), None);
}

/// Makes `link` lead to the folder `target`: a symlink on Unix, a junction on Windows (which
/// needs no right to make links).
fn folder_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    write::symlink(target, link).unwrap();
    #[cfg(windows)]
    clauseo_sandbox::windows::test_junction::junction(link, target).unwrap();
}

/// A search from the session's own work folder (in the data folder) is refused when a link
/// below it leads to another matter or into the data folder's fenced places, and goes on when
/// the link stays in the session's own folder. The search tools follow no link
/// (`links.rs`), but the fence does not rely on that.
#[test]
fn a_search_meeting_a_link_into_a_fenced_place_is_refused() {
    let setup = setup("fence-links");
    let work = setup.data.join("run").join("r1").join("work");
    for name in ["a", "b", "c", "notes"] {
        write::create_dir_all(&work.join(name)).unwrap();
    }
    folder_link(&setup.other, &work.join("a").join("jones"));
    folder_link(&setup.data, &work.join("b").join("data"));
    folder_link(&work.join("notes"), &work.join("c").join("notes"));
    let grep = |folder: &str| json!({"pattern": "lease", "path": work.join(folder)});
    let glob = |folder: &str| json!({"pattern": "**/*", "path": work.join(folder)});
    let matter = Some(Denial::Holds(RootKind::Matter));
    let data = Some(Denial::Holds(RootKind::Data));
    assert_eq!(setup.check("Grep", &grep("a")), matter);
    assert_eq!(setup.check("Glob", &glob("a")), matter);
    assert_eq!(setup.check("Grep", &grep("b")), data);
    assert_eq!(setup.check("Glob", &glob("b")), data);
    assert_eq!(setup.check("Grep", &grep("c")), None);
    assert_eq!(setup.check("Glob", &glob("c")), None);
    let whole = setup.check("Grep", &json!({"pattern": "x", "path": work}));
    assert!(whole == matter || whole == data, "{whole:?}");
    // A single file is not walked; a Read through the link is judged where it lands.
    let note = work.join("c").join("notes").join("n.md");
    write::write_file(&work.join("notes").join("n.md"), b"n").unwrap();
    assert_eq!(
        setup.check("Grep", &json!({"pattern": "x", "path": note})),
        None
    );
    let lease = work.join("a").join("jones").join("lease.pdf");
    assert_eq!(
        setup.check("Read", &file(&lease)),
        Some(Denial::Fenced(RootKind::Matter))
    );
}

/// The cost of the walk on a large matter folder: printed for the record.
#[test]
fn the_link_walk_cost_on_a_large_matter() {
    let setup = setup("fence-walk-cost");
    let (folders, files) = (100_u32, 50_u32);
    for folder in 0..folders {
        let at = setup.matter.join(format!("Folder {folder}"));
        write::create_dir_all(&at).unwrap();
        for file in 0..files {
            write::write_file(&at.join(format!("Document {file}.docx")), b"x").unwrap();
        }
    }
    let grep = json!({"pattern": "x", "path": setup.matter});
    let rounds = 20_u32;
    let start = SystemClock.now().mono;
    for _ in 0..rounds {
        assert_eq!(setup.check("Grep", &grep), None);
    }
    let mean = SystemClock.now().mono.saturating_sub(start) / rounds;
    let entries = folders * (files + 1);
    eprintln!("fence walk: {entries} entries, mean {mean:?} per Grep over {rounds} calls");
    assert!(mean < DEADLINE / 4, "{mean:?}");
}

/// Windows: a real junction inside the matter, to another matter and to the data folder (made
/// with the project's junction helper, as `mklink /J` does). Read, Grep and Glob through it are
/// refused, and so is a search from the matter folder that holds it.
#[cfg(windows)]
#[test]
fn a_junction_from_the_matter_into_a_fenced_place_is_refused() {
    let setup = setup("fence-junction");
    let jones = setup.matter.join("Jones shortcut");
    let data = setup.matter.join("Data shortcut");
    clauseo_sandbox::windows::test_junction::junction(&jones, &setup.other).unwrap();
    clauseo_sandbox::windows::test_junction::junction(&data, &setup.data).unwrap();
    let matter_hit = Some(Denial::Fenced(RootKind::Matter));
    let data_hit = Some(Denial::Fenced(RootKind::Data));
    assert_eq!(
        setup.check("Read", &file(&jones.join("lease.pdf"))),
        matter_hit
    );
    assert_eq!(
        setup.check("Read", &file(&data.join("state.sqlite"))),
        data_hit
    );
    let holds_matter = Some(Denial::Holds(RootKind::Matter));
    let holds_data = Some(Denial::Holds(RootKind::Data));
    assert_eq!(
        setup.check("Grep", &json!({"pattern": "x", "path": jones})),
        holds_matter
    );
    assert_eq!(
        setup.check("Grep", &json!({"pattern": "x", "path": data})),
        holds_data
    );
    let glob = json!({"pattern": "Jones shortcut/**/*.pdf"});
    assert_eq!(setup.check("Glob", &glob), holds_matter);
    let from_matter = setup.check("Grep", &json!({"pattern": "x", "path": setup.matter}));
    assert!(
        from_matter == holds_matter || from_matter == holds_data,
        "{from_matter:?}"
    );
    let all = setup.check("Glob", &json!({"pattern": "**/*"}));
    assert!(all == holds_matter || all == holds_data, "{all:?}");
}

/// The guard's own default (`REFUSE_NETWORK_OUTSIDE_MATTERS`): a network path outside the
/// matters is refused, untouched.
#[test]
fn the_guard_refuses_a_network_path_outside_the_matters_by_default() {
    let setup = setup("fence-network");
    let path = if cfg!(windows) {
        r"\\server\docs\precedent.docx"
    } else {
        // unix-paths-ok: a network form by its text, never opened
        "/net/server/docs/precedent.docx"
    };
    let read = json!({"file_path": path});
    assert_eq!(setup.check("Read", &read), Some(Denial::Network));
}
