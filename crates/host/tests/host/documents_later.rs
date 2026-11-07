//! The default matters folder coming after the start (`HostConfig::with_documents_later`), as
//! the desktop's Documents lookup gives it: the host starts while the lookup is held (a share
//! that does not answer), what needs the default location waits for the answer and then runs,
//! the wait is bounded and shutdown answers it, a pick or a register is checked the same before
//! the answer as after, and with no Documents folder issue #365's way stays.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use clauseo_protocol::{
    CommandEnvelope, CommandId, CommandOutcome, ErrorCode, HostCommand, HostError, HostQuery,
    MatterKind, MatterSource, MatterSummary, Principal, QueryResult, Title,
};
use clauseo_settings::Locations;
use futures::channel::oneshot;

use crate::matters::created;
use crate::support::{Dir, Running, WATCHDOG, command_id, wait};
use crate::write;

/// How long a held reply is watched for an answer that must not come.
const HELD: Duration = Duration::from_millis(100);

/// The host's bound on the wait (`documents.rs`, `HOLD_BOUND`).
const HOLD_BOUND: Duration = Duration::from_secs(5);

type Answer<T> = mpsc::Receiver<Result<T, HostError>>;

/// Starts run 0 without Documents, its answer to come on the returned sender.
fn start_held(dir: &Dir) -> (Running, oneshot::Sender<Locations>) {
    let (answer, later) = oneshot::channel();
    let home = dir.home().to_owned();
    let host = dir.start_without_documents_then(0, move |config| {
        config
            .with_home_folder(Some(home))
            .with_documents_later(later)
    });
    (host, answer)
}

fn create(name: &str, source: MatterSource) -> HostCommand {
    HostCommand::CreateMatter {
        name: Title::new(name).unwrap(),
        source,
        client: None,
        next_date: None,
    }
}

/// `query` sent on a thread of its own: the receiver hears its answer.
fn ask(host: &Running, query: HostQuery) -> Answer<QueryResult> {
    let handle = host.handle.clone();
    let (told, answer) = mpsc::channel();
    std::thread::spawn(move || {
        let asked = handle.query(Principal::LocalDesktop, query);
        let _gone = told.send(futures::executor::block_on(asked));
    });
    answer
}

/// A `CreateMatter` named `name` in the default location under `id`, sent on a thread of its
/// own: the receiver hears its answer.
fn send_create(host: &Running, id: CommandId, name: &str) -> Answer<CommandOutcome> {
    let handle = host.handle.clone();
    let envelope = CommandEnvelope {
        command_id: id,
        command: create(name, MatterSource::DefaultLocation),
    };
    let (told, answer) = mpsc::channel();
    std::thread::spawn(move || {
        let _gone = told.send(futures::executor::block_on(handle.dispatch(envelope)));
    });
    answer
}

/// The answer `answer` hears, failing when none comes within the watchdog.
fn heard<T>(answer: &Answer<T>) -> Result<T, HostError> {
    answer.recv_timeout(WATCHDOG).expect("no answer")
}

/// Asserts `answer` hears nothing for a while: it is held.
fn held<T>(answer: &Answer<T>, what: &str) {
    assert!(answer.recv_timeout(HELD).is_err(), "{what} answered");
}

fn scratch(host: &Running) -> Option<MatterSummary> {
    host.matters()
        .into_iter()
        .find(|matter| matter.kind == MatterKind::Scratch)
}

/// Waits for the Scratch matter to be listed.
fn wait_for_scratch(host: &Running) -> MatterSummary {
    for _ in 0..2_000 {
        if let Some(matter) = scratch(host) {
            return matter;
        }
        wait("a tick", async_io::Timer::after(Duration::from_millis(5)));
    }
    panic!("no Scratch matter");
}

fn marker(folder: &Path) -> PathBuf {
    folder.join(".clauseo").join("matter.json")
}

fn support_query() -> HostQuery {
    HostQuery::FolderSupport {
        source: MatterSource::DefaultLocation,
    }
}

/// Asserts `error` is the retryable refusal for a lookup that has not answered.
fn assert_not_yet(error: &HostError) {
    assert_eq!(error.code(), ErrorCode::MatterFolderMissing, "{error}");
    assert!(error.code().is_retryable(), "{error}");
    assert!(error.message().contains("not answered yet"), "{error}");
}

/// The host starts while the Documents lookup is held (it answers only after `Host::start`
/// returned, so the start cannot have waited on it). Meanwhile a create, a folder check and
/// `GetDefaultLocation` in the default location wait, and a create resent under its id joins
/// the first; the answer runs them, the create once, in the folder it names, and Scratch is
/// made there.
#[test]
fn what_needs_the_default_location_waits_for_documents_and_then_runs() {
    let dir = Dir::new("documents_later", &[]);
    let (host, answer) = start_held(&dir);
    assert_eq!(scratch(&host), None);
    let id = command_id();
    let first = send_create(&host, id, "Acme");
    let check = ask(&host, support_query());
    let location = ask(&host, HostQuery::GetDefaultLocation);
    held(&first, "the create");
    let again = send_create(&host, id, "Acme");
    held(&again, "the resent create");
    held(&check, "the folder check");
    held(&location, "GetDefaultLocation");
    assert_eq!(host.matters(), [], "nothing made before the answer");
    answer.send(Locations::under_home(dir.home())).unwrap();
    let matter = match heard(&first).unwrap() {
        CommandOutcome::MatterCreated { matter } => matter,
        other => panic!("CreateMatter answered {other:?}"),
    };
    assert_eq!(
        heard(&again).unwrap(),
        CommandOutcome::MatterCreated { matter }
    );
    heard(&check).unwrap();
    match heard(&location).unwrap() {
        QueryResult::DefaultLocation(location) => assert_eq!(
            location.display,
            Path::new("Documents").join("Clauseo").display().to_string()
        ),
        other => panic!("GetDefaultLocation answered {other:?}"),
    }
    let base = dir.home().join("Documents").join("Clauseo");
    assert!(marker(&base.join("Acme")).is_file());
    wait_for_scratch(&host);
    assert!(marker(&base.join("Scratch")).is_file());
    let folders = host.matters().len();
    assert_eq!(folders, 2, "Acme once, and Scratch");
    host.shutdown();
}

/// No refusal reads Documents: before the answer a new folder inside the data folder and an
/// import of the data folder itself are refused as after it, and a fine folder is taken.
#[test]
fn a_pick_before_documents_answers_is_checked_as_after() {
    let dir = Dir::new("documents_later_checks", &[]);
    let (host, answer) = start_held(&dir);
    let inside = dir.data().join("work");
    write::create_dir_all(&inside);
    let refusals = |host: &Running| {
        let inside = host.folder_ref(&inside);
        host.refused(
            create("Inside", MatterSource::NewFolder { folder: inside }),
            ErrorCode::MatterFolderInMattersRoot,
        );
        let data = host.folder_ref(dir.data());
        host.refused(
            create("Data", MatterSource::ExistingFolder { folder: data }),
            ErrorCode::MatterFolderInMattersRoot,
        );
    };
    refusals(&host);
    let parent = dir.folder("parent", &[]);
    let folder = host.folder_ref(&parent);
    created(host.dispatch(create("Fine", MatterSource::NewFolder { folder })));
    assert!(marker(&parent.join("Fine")).is_file());
    answer.send(Locations::under_home(dir.home())).unwrap();
    wait_for_scratch(&host);
    refusals(&host);
    host.shutdown();
}

/// A lookup that finds no Documents folder keeps issue #365's way: what waited gets
/// `command.invalid` (no default location), there is no Scratch, and so does a later create.
#[test]
fn an_answer_without_documents_gives_no_default_location() {
    let dir = Dir::new("documents_later_none", &[]);
    let (host, answer) = start_held(&dir);
    let waiting = send_create(&host, command_id(), "Acme");
    let location = ask(&host, HostQuery::GetDefaultLocation);
    held(&waiting, "the create");
    let none = Locations::under_home(dir.home()).without_default_matters_dir();
    answer.send(none).unwrap();
    for error in [heard(&waiting).unwrap_err(), heard(&location).unwrap_err()] {
        assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
        assert!(error.message().contains("no default location"), "{error}");
    }
    assert_eq!(scratch(&host), None);
    host.refused(
        create("Later", MatterSource::DefaultLocation),
        ErrorCode::CommandInvalid,
    );
    assert_eq!(host.matters(), []);
    host.shutdown();
}

/// Past the bound on the host clock, what waited is refused as retryable, and so is a create
/// sent before the answer; a late answer still fills the default location in. A lookup whose
/// sender is dropped refuses what waited the same way, then leaves no default location.
#[test]
fn the_wait_is_bounded_and_fails_safe() {
    let dir = Dir::new("documents_later_bound", &[]);
    let (host, answer) = start_held(&dir);
    let waiting = send_create(&host, command_id(), "Acme");
    let check = ask(&host, support_query());
    held(&waiting, "the create");
    host.clock.advance(HOLD_BOUND);
    assert_not_yet(&heard(&waiting).unwrap_err());
    assert_not_yet(&heard(&check).unwrap_err());
    let refused = host.refused(
        create("Acme", MatterSource::DefaultLocation),
        ErrorCode::MatterFolderMissing,
    );
    assert_not_yet(&refused);
    answer.send(Locations::under_home(dir.home())).unwrap();
    wait_for_scratch(&host);
    created(host.dispatch(create("Acme", MatterSource::DefaultLocation)));
    host.shutdown();

    let dir = Dir::new("documents_later_dropped", &[]);
    let (host, answer) = start_held(&dir);
    let waiting = send_create(&host, command_id(), "Acme");
    held(&waiting, "the create");
    drop(answer);
    assert_not_yet(&heard(&waiting).unwrap_err());
    host.refused(
        create("Acme", MatterSource::DefaultLocation),
        ErrorCode::CommandInvalid,
    );
    host.shutdown();
}

/// Shutdown answers what waits, and does not wait for the lookup.
#[test]
fn shutdown_answers_what_waits() {
    let dir = Dir::new("documents_later_shutdown", &[]);
    let (host, answer) = start_held(&dir);
    let waiting = send_create(&host, command_id(), "Acme");
    let location = ask(&host, HostQuery::GetDefaultLocation);
    held(&waiting, "the create");
    host.shutdown();
    assert!(heard(&waiting).is_err());
    assert!(heard(&location).is_err());
    let gone = answer.send(Locations::under_home(dir.home()));
    assert!(gone.is_err(), "the host still waited for the lookup");
}
