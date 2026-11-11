//! Tracked changes with the real stetdoc, when one is on `PATH` (a debug build's app finds it
//! the same way; CI may have none, and the test then says so and passes). The Hindi letter
//! (`fixtures/hindi/patra.docx`) gets one tracked replacement from stetdoc itself in the setup;
//! the host lists it, accepts the insertion alone, and stetdoc then reads the file as written:
//! one change left, and the new text part of the document even before the remaining deletion
//! is decided.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_host::HostConfig;
use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions};
use clauseo_profile::skills::stetdoc::{RunOptions, run_for};
use clauseo_protocol::{
    CommandOutcome, DecideRevisions, HostCommand, HostQuery, MatterId, MatterPath, QueryResult,
    RevisionDecision, RevisionKind, RevisionList,
};

use crate::support::{Dir, FAKE, Running, Scenario};
use crate::write;

/// `patra.docx`: the letter as `generate.py` writes it.
const ORIGINAL: &[u8] = include_bytes!("../fixtures/hindi/patra.docx");

/// Text of the letter's third paragraph, and what the setup replaces it with.
const OLD: &str = "अंतिम फ़ैसला";
const NEW: &str = "निर्णय";

/// The first `stetdoc` on `PATH`, as `which` finds it.
#[expect(
    clippy::disallowed_methods,
    reason = "the test looks for a stetdoc the machine has; it may have none"
)]
fn stetdoc_on_path() -> Option<PathBuf> {
    let name = format!("stetdoc{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&name))
        .find(|program| program.is_absolute() && program.is_file())
}

/// stetdoc's stdout for `args`, which must exit 0.
fn stetdoc(program: &Path, args: &[&str]) -> String {
    let launcher = PassthroughLauncher::new(SpawnOptions::new().clear_env());
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let ran = futures::executor::block_on(run_for(
        &launcher,
        &AsyncIoTimer,
        program,
        &args,
        &RunOptions::new(Duration::from_secs(10)),
    ))
    .unwrap();
    let stdout = String::from_utf8(ran.stdout).unwrap();
    assert_eq!(ran.code, Some(0), "{stdout}{}", ran.stderr_tail);
    stdout
}

fn list(host: &Running, matter: MatterId, path: &MatterPath) -> RevisionList {
    let query = HostQuery::ListRevisions {
        matter,
        path: path.clone(),
    };
    match host.query(query).unwrap() {
        QueryResult::Revisions(list) => list,
        other => panic!("not revisions: {other:?}"),
    }
}

#[test]
fn the_real_stetdoc_lists_and_accepts_a_tracked_change() {
    let Some(program) = stetdoc_on_path() else {
        eprintln!("skipped: no stetdoc on PATH");
        return;
    };
    let dir = Dir::new(
        "revisions-real",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let folder = dir.work_path("letter");
    let file = folder.join("patra.docx");
    write::write(&file, ORIGINAL);
    let file_text = file.to_str().unwrap();
    stetdoc(
        &program,
        &[
            "replace",
            file_text,
            OLD,
            NEW,
            "--in-place",
            "--author",
            "Clauseo",
        ],
    );
    let host = dir.start_with(0, |config: HostConfig| {
        config.with_stetdoc(program.clone(), "Clauseo".to_owned())
    });
    let matter = host.create_matter("letter", &folder);
    let path = MatterPath::new("patra.docx").unwrap();

    let before = list(&host, matter, &path);
    assert!(!before.truncated);
    let kinds: Vec<_> = before.revisions.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [RevisionKind::Delete, RevisionKind::Insert],
        "{before:?}"
    );
    for revision in &before.revisions {
        assert_eq!(revision.author, "Clauseo");
        assert_eq!(revision.block, "p3");
        assert!(revision.at.is_some(), "{revision:?}");
    }
    assert_eq!(before.revisions[0].text.as_deref(), Some(OLD));
    assert_eq!(before.revisions[1].text.as_deref(), Some(NEW));

    let insertion = before.revisions[1].ids.clone();
    let outcome = host.ok(HostCommand::DecideRevisions(DecideRevisions {
        matter,
        path: path.clone(),
        version: before.version,
        ids: insertion,
        decision: RevisionDecision::Accept,
    }));
    assert!(
        matches!(outcome, CommandOutcome::RevisionsDecided { .. }),
        "{outcome:?}"
    );

    let after = list(&host, matter, &path);
    assert_ne!(after.version, before.version);
    let left: Vec<_> = after.revisions.iter().map(|r| r.kind).collect();
    assert_eq!(left, [RevisionKind::Delete], "{after:?}");
    let final_text = stetdoc(&program, &["view", file_text, "--mode", "final"]);
    assert!(final_text.contains(NEW), "{final_text}");
    assert!(!final_text.contains(OLD), "{final_text}");
    // The insertion is plain text now: the document before its tracked changes holds it too.
    let original_text = stetdoc(&program, &["view", file_text, "--mode", "original"]);
    assert!(original_text.contains(NEW), "{original_text}");
    host.shutdown();
}
