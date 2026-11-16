//! The bound on `open_file`'s and `reveal`'s blocking work (`bounded.rs`): a marker check held
//! past it, as on a share that does not answer, answers `matter.folder_missing` (retryable, no
//! path) while the check is still held, so the caller's thread is not; nothing opens.

use std::sync::mpsc;

use super::*;

/// Far shorter than [`FOLDER_WAIT`], so the test waits only this long; the check it holds never
/// returns before the test releases it.
const SHORT: Duration = Duration::from_millis(50);

/// A host whose marker check blocks until the returned sender is dropped.
fn held(host: &mut FakeHost) -> mpsc::Sender<()> {
    let (release, wait) = mpsc::channel::<()>();
    let wait = Mutex::new(wait);
    host.check = Arc::new(move |_: &Path, _| {
        let _released = wait.lock().unwrap().recv();
        Ok(())
    });
    release
}

fn assert_no_answer(refused: Result<impl std::fmt::Debug, HostError>, path: &Path) {
    let refused = refused.unwrap_err();
    assert_eq!(
        refused.code(),
        ErrorCode::MatterFolderMissing,
        "{refused:?}"
    );
    assert!(refused.retryable());
    assert_eq!(refused.message(), crate::open::bounded::NO_ANSWER);
    assert!(!refused.message().contains(&*path.to_string_lossy()));
}

#[test]
fn a_held_marker_check_answers_folder_missing_without_holding_the_caller() {
    let scratch = Scratch::new("open-held", &[]);
    let (folder, brief, _) = matter_folder(&scratch);
    let mut host = host(&folder, &brief);
    let release = held(&mut host);
    let opener = Arc::new(FakeOpener::default());
    let matter = RevealTarget::Matter(host.located.matter);

    let started = gate::now();
    let refused = block_on(reveal(&host, &opener, scratch.path(), SHORT, matter));
    assert_no_answer(refused, &folder);
    let gate = gate::Gate::default();
    let clock = Clock::new();
    let now = || clock.now();
    let refused = block_on(open_file(&host, &opener, &gate, &now, SHORT, host.file));
    assert_no_answer(refused, &folder);
    // Both answered while their checks are still held (the sender is alive), well inside the
    // wait a held check on the caller's thread would take.
    assert!(gate::now().duration_since(started) < Duration::from_secs(5));
    assert_eq!(opener.opened(), []);
    drop(release);

    // Released, the same calls go ahead; the late checks, finishing now, open nothing.
    host.refusing(None);
    block_on(reveal(&host, &opener, scratch.path(), SHORT, matter)).unwrap();
    assert_eq!(opener.opened(), [Opened::Reveal(folder)]);
}

#[test]
fn bounded_work_answers_its_own_answer_and_a_panic_is_internal() {
    let answer = block_on(crate::open::bounded::bounded(
        "test",
        FOLDER_WAIT,
        ErrorCode::MatterFolderMissing,
        || Ok(7),
    ));
    assert_eq!(answer, Ok(7));
    let panicked: Result<(), HostError> = block_on(crate::open::bounded::bounded(
        "test",
        FOLDER_WAIT,
        ErrorCode::MatterFolderMissing,
        || panic!("the work panicked"),
    ));
    assert_eq!(panicked.unwrap_err().code(), ErrorCode::Internal);
}
