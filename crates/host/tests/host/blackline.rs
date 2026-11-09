//! The Word blackline (`HostQuery::WordBlackline`) through the host, with a stand-in stetdoc
//! (`examples/stetdoc_compare_fake.rs`) that answers the fixture's view
//! (`tests/fixtures/blackline/`): the answer, the refusals (`blackline.not_word`,
//! `blackline.no_word_tool`), the failures (`blackline.failed`: compare exits 1, the view takes
//! too long, the view prints too much), and after every case `<data>/tmp` holds nothing and the
//! matter folder is as it was. One test runs the real stetdoc when one is on `PATH`, to a
//! version and to the folder now.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_process::{Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{
    CheckpointId, CheckpointKind, ErrorCode, HostError, HostQuery, MatterId, MatterPath,
    ParagraphChange, Principal, QueryResult, RunKind, StreamId, WordBlackline,
};

use crate::support::{Dir, FAKE, Running, START_MS, Scenario, matter_with_thread, wait};
use crate::versions::{matter_events, recorded};
use crate::write;

/// The fixtures' folder.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/blackline")
}

/// A fixture's bytes.
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

/// The document every test compares.
const DOC: &str = "Lease.docx";

/// The stand-in stetdoc copied into `<root>/fake-stetdoc/`, with the fixture's `view.json`
/// beside it.
fn fake_stetdoc(dir: &Dir) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let examples = exe.parent().unwrap().parent().unwrap().join("examples");
    let name = format!("stetdoc_compare_fake{}", std::env::consts::EXE_SUFFIX);
    let folder = dir.home().join("fake-stetdoc");
    write::create_dir_all(&folder);
    let target = folder.join(&name);
    let built = std::fs::read(examples.join(&name))
        .unwrap_or_else(|e| panic!("{e}: build it with cargo build -p clauseo-host --examples"));
    write::write(&target, &built);
    #[cfg(unix)]
    write::make_executable(&target);
    write::write(&folder.join("view.json"), &fixture("view.json"));
    target
}

/// A host with `stetdoc` (none when `None`), and a matter whose `Lease.docx` was the fixture's
/// `old.docx` at its import and is `new.docx` now. Returns the host, the matter, its import
/// version and its folder.
fn lease_matter(dir: &Dir, stetdoc: Option<&Path>) -> (Running, MatterId, CheckpointId, PathBuf) {
    lease_matter_launching(dir, stetdoc, Arc::new(dir.launcher()))
}

/// As [`lease_matter`], starting every child (stetdoc included) through `launcher`.
fn lease_matter_launching(
    dir: &Dir,
    stetdoc: Option<&Path>,
    launcher: Arc<dyn Launcher>,
) -> (Running, MatterId, CheckpointId, PathBuf) {
    let host = dir.start_launching(0, launcher, |config| match stetdoc {
        Some(program) => config.with_stetdoc(program.to_owned(), "Clauseo".to_owned()),
        None => config,
    });
    let (matter, _, folder) = matter_with_thread(dir, &host, "m", &[], FAKE);
    // The import saw an empty folder; the file comes before a version that holds it.
    write::write(&folder.join(DOC), &fixture("old.docx"));
    let thread = host.threads(matter)[0].thread;
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the lease.");
    events.until_turn_ended(turn);
    let old = last_version(&host, matter);
    write::write(&folder.join(DOC), &fixture("new.docx"));
    (host, matter, old, folder)
}

/// The matter's newest version.
fn last_version(host: &Running, matter: MatterId) -> CheckpointId {
    recorded(&matter_events(host, matter)).last().unwrap().0
}

/// The blackline query of `path` from `from` to `to`.
fn query(matter: MatterId, path: &str, from: CheckpointId, to: Option<CheckpointId>) -> HostQuery {
    HostQuery::WordBlackline {
        matter,
        path: MatterPath::new(path).unwrap(),
        from,
        to,
    }
}

/// The blackline of an answer.
fn blackline(answer: Result<QueryResult, HostError>) -> WordBlackline {
    match answer {
        Ok(QueryResult::WordBlackline(blackline)) => blackline,
        other => panic!("WordBlackline answered {other:?}"),
    }
}

/// What `<data>/tmp` holds: nothing once every blackline is done.
fn temp_entries(dir: &Dir) -> Vec<PathBuf> {
    match std::fs::read_dir(dir.data().join("tmp")) {
        Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Every file of `folder` but Clauseo's own `.clauseo/`, with its bytes, sorted.
fn files_of(folder: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() != ".clauseo" {
            let bytes = std::fs::read(&path).unwrap_or_default();
            files.push((path, bytes));
        }
    }
    files.sort();
    files
}

/// Fails unless `answer` is refused with `code`.
fn refused(answer: Result<QueryResult, HostError>, code: ErrorCode) {
    match answer {
        Err(error) => assert_eq!(error.code(), code, "{error}"),
        Ok(other) => panic!("expected {code:?}, got {other:?}"),
    }
}

#[test]
fn a_blackline_answers_the_paragraphs_and_leaves_nothing_behind() {
    let dir = Dir::new(
        "blackline_answers",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let stetdoc = fake_stetdoc(&dir);
    let (host, matter, old, folder) = lease_matter(&dir, Some(&stetdoc));
    let before = files_of(&folder);

    let answer = blackline(host.query(query(matter, DOC, old, None)));
    assert!(!answer.truncated);
    assert_eq!(answer.paragraphs.len(), 15);
    let second = &answer.paragraphs[1];
    assert_eq!(second.change, ParagraphChange::Changed);
    let runs: Vec<(RunKind, &str)> = second
        .runs
        .iter()
        .map(|run| (run.kind, run.text.as_str()))
        .collect();
    assert_eq!(
        runs,
        [
            (RunKind::Same, "The Tenant shall pay rent of $1,"),
            (RunKind::Del, "000 monthly"),
            (RunKind::Ins, "200 each month in advance"),
            (RunKind::Same, "."),
        ]
    );
    assert_eq!(answer.paragraphs[2].change, ParagraphChange::Removed);
    assert_eq!(answer.paragraphs[13].change, ParagraphChange::Added);

    // stetdoc got the version's copy and the folder's, by the author given, in the temp folder.
    let fake = stetdoc.parent().unwrap();
    assert_eq!(
        std::fs::read(fake.join("seen-old.docx")).unwrap(),
        fixture("old.docx")
    );
    assert_eq!(
        std::fs::read(fake.join("seen-new.docx")).unwrap(),
        fixture("new.docx")
    );
    let args = std::fs::read_to_string(fake.join("args-compare.txt")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert_eq!(args[..4], ["compare", "--json", "--author=Clauseo", "-o"]);
    let tmp = dir.data().join("tmp");
    for (arg, name) in args[4..]
        .iter()
        .zip(["redline.docx", "old.docx", "new.docx"])
    {
        let arg = Path::new(arg);
        assert_eq!(arg.file_name().unwrap(), name);
        assert_eq!(arg.parent().unwrap().parent().unwrap(), tmp);
        let folder = arg.parent().unwrap().file_name().unwrap().to_str().unwrap();
        assert!(folder.starts_with("blackline-"), "{folder}");
    }
    let view = std::fs::read_to_string(fake.join("args-view.txt")).unwrap();
    assert_eq!(
        view.lines().take(3).collect::<Vec<_>>(),
        ["view", "--all", "--json"]
    );

    assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
    assert_eq!(files_of(&folder), before);
    host.shutdown();
}

#[test]
fn a_file_that_is_not_a_docx_is_not_word() {
    let dir = Dir::new(
        "blackline_not_word",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let stetdoc = fake_stetdoc(&dir);
    let (host, matter, old, folder) = lease_matter(&dir, Some(&stetdoc));
    let before = files_of(&folder);
    refused(
        host.query(query(matter, "notes.txt", old, None)),
        ErrorCode::BlacklineNotWord,
    );
    refused(
        host.query(query(matter, "Lease.doc", old, None)),
        ErrorCode::BlacklineNotWord,
    );
    // `.DOCX` is a Word document: past that check, a file the version lacks is refused as
    // `CompareVersions` refuses one, and the temp folder made for it is gone.
    refused(
        host.query(query(matter, "Other.DOCX", old, None)),
        ErrorCode::CommandInvalid,
    );
    assert!(!stetdoc.parent().unwrap().join("args-compare.txt").exists());
    assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
    assert_eq!(files_of(&folder), before);
    host.shutdown();
}

#[test]
fn a_host_without_stetdoc_has_no_word_tool() {
    let dir = Dir::new(
        "blackline_no_tool",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let (host, matter, old, _) = lease_matter(&dir, None);
    refused(
        host.query(query(matter, DOC, old, None)),
        ErrorCode::BlacklineNoWordTool,
    );
    assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
    host.shutdown();
}

#[test]
fn a_compare_that_exits_1_fails() {
    let dir = Dir::new(
        "blackline_compare_exit",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let stetdoc = fake_stetdoc(&dir);
    write::write(&stetdoc.parent().unwrap().join("compare-exit.txt"), b"1");
    let (host, matter, old, folder) = lease_matter(&dir, Some(&stetdoc));
    let before = files_of(&folder);
    let error = host.query(query(matter, DOC, old, None)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::BlacklineFailed, "{error}");
    // The client gets a fixed message; what failed is in the log only.
    assert_eq!(
        error.message(),
        "the Word blackline failed; the detail is in the host's log"
    );
    assert!(!stetdoc.parent().unwrap().join("args-view.txt").exists());
    assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
    assert_eq!(files_of(&folder), before);
    host.shutdown();
}

#[test]
fn a_view_that_takes_too_long_fails() {
    let dir = Dir::new(
        "blackline_view_hangs",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let stetdoc = fake_stetdoc(&dir);
    write::write(&stetdoc.parent().unwrap().join("view-mode.txt"), b"hang");
    let (host, matter, old, folder) = lease_matter(&dir, Some(&stetdoc));
    let before = files_of(&folder);
    let handle = host.handle.clone();
    let asked = query(matter, DOC, old, None);
    let (send, answer) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _gone = send.send(wait(
            "the blackline",
            handle.query(Principal::LocalDesktop, asked),
        ));
    });
    // Once the view runs (it notes its arguments first), the host clock passes its 30 s; the
    // view never answers. The clock moves again until the answer comes, in case the view's
    // timer was armed after a move.
    let started = stetdoc.parent().unwrap().join("args-view.txt");
    let mut answered = None;
    for _ in 0..400 {
        if started.exists() {
            host.clock.advance(Duration::from_secs(30));
        }
        if let Ok(got) = answer.recv_timeout(Duration::from_millis(50)) {
            answered = Some(got);
            break;
        }
    }
    refused(
        answered.expect("the blackline never answered"),
        ErrorCode::BlacklineFailed,
    );
    assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
    assert_eq!(files_of(&folder), before);
    host.shutdown();
}

#[test]
fn a_view_past_8_mib_fails() {
    let dir = Dir::new(
        "blackline_view_big",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let stetdoc = fake_stetdoc(&dir);
    write::write(&stetdoc.parent().unwrap().join("view-mode.txt"), b"big");
    let (host, matter, old, _) = lease_matter(&dir, Some(&stetdoc));
    refused(
        host.query(query(matter, DOC, old, None)),
        ErrorCode::BlacklineFailed,
    );
    assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
    host.shutdown();
}

/// A blackline folder's name for an id minted at `millis`.
fn blackline_dir_at(millis: i64) -> String {
    let millis = u64::try_from(millis).unwrap();
    let id = uuid::Builder::from_unix_timestamp_millis(millis, &[7; 10]).into_uuid();
    format!("blackline-{}", id.simple())
}

/// A crash or quit while stetdoc ran leaves its folder: the next start removes the blackline
/// folders of earlier runs from `<data>/tmp`, and leaves everything else there, a folder named
/// for this run's start included.
#[test]
fn the_next_start_removes_leftover_blackline_folders_and_nothing_else() {
    let dir = Dir::new(
        "blackline_sweep",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let tmp = dir.data().join("tmp");
    let earlier = tmp.join(blackline_dir_at(START_MS - 60_000));
    let not_an_id = tmp.join("blackline-left-by-hand");
    let this_run = tmp.join(blackline_dir_at(START_MS));
    let other = tmp.join("other-folder");
    let file = tmp.join("blackline-a-file");
    for folder in [&earlier, &not_an_id, &this_run, &other] {
        write::create_dir_all(folder);
        write::write(&folder.join("old.docx"), b"x");
    }
    write::write(&file, b"x");

    let host = dir.start(0);
    wait("the sweep", async {
        while earlier.exists() || not_an_id.exists() {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
    host.settle();
    assert!(this_run.join("old.docx").exists());
    assert!(other.join("old.docx").exists());
    assert!(file.exists());
    host.shutdown();
}

/// The first `stetdoc` on `PATH`, if any.
#[expect(
    clippy::disallowed_methods,
    reason = "the real-stetdoc test looks stetdoc up on PATH, as a debug desktop build does; the \
              host itself never does"
)]
fn stetdoc_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let name = format!("stetdoc{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&path)
        .map(|folder| folder.join(&name))
        .find(|program| program.is_absolute() && program.is_file())
}

#[test]
fn the_real_stetdoc_blacklines_a_version_and_the_folder_now() {
    let Some(stetdoc) = stetdoc_on_path() else {
        eprintln!("skipped: no stetdoc on PATH");
        return;
    };
    let dir = Dir::new(
        "blackline_real",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    // As the app's host launcher does (`host_start.rs`): the environment cleared, so stetdoc
    // runs with nothing but its spec's (empty) environment.
    let launcher = PassthroughLauncher::new(
        SpawnOptions::new()
            .home(dir.home().join("home"))
            .clear_env(),
    );
    let (host, matter, old, folder) =
        lease_matter_launching(&dir, Some(&stetdoc), Arc::new(launcher));
    let before = files_of(&folder);
    let check = |answer: WordBlackline| {
        assert!(!answer.truncated);
        let changes: Vec<ParagraphChange> = answer.paragraphs.iter().map(|p| p.change).collect();
        assert_eq!(changes.len(), 15, "{answer:?}");
        assert_eq!(changes[0], ParagraphChange::Same);
        assert_eq!(changes[1], ParagraphChange::Changed);
        assert_eq!(changes[2], ParagraphChange::Removed);
        assert_eq!(changes[13], ParagraphChange::Added);
    };
    check(blackline(host.query(query(matter, DOC, old, None))));

    // A version that holds the new text: the next turn's checkpoint takes the folder as it is.
    let thread = host.threads(matter)[0].thread;
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read it again.");
    events.until_turn_ended(turn);
    let new = last_version(&host, matter);
    assert_ne!(new, old);
    assert!(
        matter_events(&host, matter)
            .checkpoint(CheckpointKind::Import)
            .is_some()
    );
    check(blackline(host.query(query(matter, DOC, old, Some(new)))));

    assert_eq!(temp_entries(&dir), Vec::<PathBuf>::new());
    assert_eq!(files_of(&folder), before);
    host.shutdown();
}
