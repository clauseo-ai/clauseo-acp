//! A new matter folder's probe off the actor, held by a stand-in the test releases: the host
//! answers other calls while it is held, a probe that does not answer within [`PROBE_WAIT`] on
//! the manual clock fails its create with `matter.folder_missing`, two creates of one name
//! make one matter, and a host stopped mid-probe makes none. Never a sleep.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{
    CommandEnvelope, CommandId, CommandOutcome, ErrorCode, FileRefId, HostCommand, HostError,
    HostQuery, MatterKind, MatterSource, Principal, QueryResult, Timestamp, Title,
};
use clauseo_settings::{Effective, Locations, SystemFiles};
use clauseo_store::{Store, StoreOptions};
use futures::executor::block_on;
use uuid::Uuid;

use super::{FolderProbes, PROBE_WAIT, probe_folder, probe_located};
use crate::pipeline::tests::support::TempDir;
use crate::{Host, HostConfig, HostHandle, HostJoin, LocatedFile, ManualClock};

/// How long a test waits for the host's threads before it fails: well inside nextest's 30 s, so
/// a probe never called fails here, saying which wait it was, rather than timing out.
const PATIENCE: Duration = Duration::from_secs(10);

/// The new matter's name.
const NAME: &str = "Smith v Jones";

/// A probe stand-in: it says when it is entered, waits for the test's release, then probes as
/// the host does and says when it has answered.
struct Held<T> {
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    answered: mpsc::Receiver<T>,
}

/// A held stand-in for `probe`, called with `A` and `B`; with `first` it probes before it is
/// held, so what it answers is what the disk held when it was entered.
fn held_stand_in<A: ?Sized, B: ?Sized, T: Clone + Send + 'static>(
    probe: fn(&A, &B) -> T,
    first: bool,
) -> (impl Fn(&A, &B) -> T + Send + Sync, Held<T>) {
    let (enter, entered) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let (answer, answered) = mpsc::channel();
    let (enter, answer, released) = (Mutex::new(enter), Mutex::new(answer), Mutex::new(released));
    let stand_in = move |a: &A, b: &B| {
        let early = first.then(|| probe(a, b));
        let _told = enter.lock().map(|enter| enter.send(()));
        let _released = released
            .lock()
            .map(|released| released.recv_timeout(PATIENCE));
        let found = early.unwrap_or_else(|| probe(a, b));
        let _told = answer.lock().map(|answer| answer.send(found.clone()));
        found
    };
    let held = Held {
        entered,
        release,
        answered,
    };
    (stand_in, held)
}

/// A started host whose folder probes are held: a new matter's, then a located file's.
fn start(dir: &Path, clock: &ManualClock) -> (HostHandle, HostJoin, Held<(PathBuf, bool)>) {
    let (host, join, held, _locate) = start_both(dir, clock);
    (host, join, held)
}

type Both = (
    HostHandle,
    HostJoin,
    Held<(PathBuf, bool)>,
    Held<(bool, bool)>,
);

fn start_both(dir: &Path, clock: &ManualClock) -> Both {
    start_with(dir, clock, false, None)
}

/// [`start_both`], the new folder probe probing `first` (`held_stand_in`), and shutdown
/// waiting for the probes running when `stop` is given, said on it each time.
fn start_with(
    dir: &Path,
    clock: &ManualClock,
    first: bool,
    stop: Option<mpsc::Sender<()>>,
) -> Both {
    crate::write::create_dir_all(&dir.join("data")).unwrap();
    let effective = Effective::load(&Locations::under_home(dir), &SystemFiles, &[]);
    let launcher = PassthroughLauncher::new(SpawnOptions::new().home(dir.join("home")));
    let (new_folder, held) = held_stand_in(probe_folder, first);
    let (locate, held_locate) = held_stand_in(probe_located, false);
    let probes = FolderProbes::new(Arc::new(new_folder), Arc::new(locate));
    let probes = match stop {
        Some(told) => probes.with_stop_held(told),
        None => probes,
    };
    let config = HostConfig::new(effective, Arc::new(launcher))
        .with_clock(Arc::new(clock.clone()))
        .with_ids(Box::new(crate::SequentialIds::with_run(0)))
        .with_folder_probes(probes);
    let (host, join) = Host::start(config).unwrap();
    (host, join, held, held_locate)
}

fn clock() -> ManualClock {
    ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap())
}

type Dispatched = JoinHandle<Result<CommandOutcome, HostError>>;

/// Dispatches command `n`, a `CreateMatter` of [`NAME`] in the default location, on its own
/// thread, as its answer waits for the probe.
fn create(host: &HostHandle, n: u128) -> Dispatched {
    let host = host.clone();
    let command = CommandEnvelope {
        command_id: CommandId::new(Uuid::from_u128(n)).unwrap(),
        command: HostCommand::CreateMatter {
            name: Title::new(NAME).unwrap(),
            source: MatterSource::DefaultLocation,
            client: None,
            next_date: None,
        },
    };
    std::thread::spawn(move || block_on(host.dispatch(command)))
}

/// The matters on the list that are not Scratch.
fn folder_matters(host: &HostHandle) -> usize {
    let listed = block_on(host.query(Principal::LocalDesktop, HostQuery::ListMatters));
    let QueryResult::Matters(matters) = listed.unwrap() else {
        panic!("not matters");
    };
    matters
        .iter()
        .filter(|matter| matter.kind == MatterKind::Folder)
        .count()
}

fn new_folder(dir: &Path) -> PathBuf {
    dir.join("Documents").join("Clauseo").join(NAME)
}

/// Waits until the probe's [`PROBE_WAIT`] timer is armed.
fn probe_wait_armed(clock: &ManualClock) {
    let armed = clock.clone();
    let (told, heard) = mpsc::channel();
    std::thread::spawn(move || {
        block_on(async move { while armed.next_armed().await != PROBE_WAIT {} });
        let _gone = told.send(());
    });
    heard
        .recv_timeout(PATIENCE)
        .expect("the probe's 10 s timer was never armed");
}

#[test]
fn a_create_matter_on_a_hung_share_leaves_the_host_answering() {
    let dir = TempDir::new("probe-hung");
    let clock = clock();
    let (host, join, held) = start(dir.path(), &clock);
    let created = create(&host, 1);
    held.entered
        .recv_timeout(PATIENCE)
        .expect("the new folder probe was never entered");

    // The actor answers while the probe is held.
    assert_eq!(folder_matters(&host), 0);
    assert!(
        !created.is_finished(),
        "the create answered with its probe held"
    );

    probe_wait_armed(&clock);
    clock.advance(PROBE_WAIT);
    let refused = created.join().unwrap().unwrap_err();
    assert_eq!(refused.code(), ErrorCode::MatterFolderMissing);
    let home = dir.path().to_string_lossy().into_owned();
    assert!(
        !refused.message().contains(&home),
        "the refusal names a path: {}",
        refused.message()
    );

    // The probe answers late: nothing is made.
    held.release.send(()).unwrap();
    assert!(
        !held
            .answered
            .recv_timeout(PATIENCE)
            .expect("the probe never answered")
            .1
    );
    assert_eq!(folder_matters(&host), 0, "a matter was stored");
    assert!(!new_folder(dir.path()).exists());
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
}

#[test]
fn a_create_matter_whose_probe_answers_creates_it() {
    let dir = TempDir::new("probe-answers");
    let clock = clock();
    let (host, join, held) = start(dir.path(), &clock);
    let created = create(&host, 1);
    held.entered
        .recv_timeout(PATIENCE)
        .expect("the new folder probe was never entered");
    held.release.send(()).unwrap();
    let outcome = created.join().unwrap().unwrap();
    assert!(matches!(outcome, CommandOutcome::MatterCreated { .. }));
    assert_eq!(folder_matters(&host), 1);
    assert!(new_folder(dir.path()).is_dir());
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
}

#[test]
fn two_creates_of_the_same_name_while_the_probe_waits_make_one_matter() {
    let dir = TempDir::new("probe-twice");
    let clock = clock();
    let (host, join, held, _locate) = start_with(dir.path(), &clock, true, None);
    let first = create(&host, 1);
    let second = create(&host, 2);
    held.entered
        .recv_timeout(PATIENCE)
        .expect("the new folder probe was never entered");
    held.entered
        .recv_timeout(PATIENCE)
        .expect("the new folder probe was never entered");
    // Both probes find no folder: neither create has made it yet.
    held.release.send(()).unwrap();
    held.release.send(()).unwrap();
    assert!(
        !held
            .answered
            .recv_timeout(PATIENCE)
            .expect("the probe never answered")
            .1
    );
    assert!(
        !held
            .answered
            .recv_timeout(PATIENCE)
            .expect("the probe never answered")
            .1
    );

    let answers = [first.join().unwrap(), second.join().unwrap()];
    let made = answers.iter().filter(|answer| answer.is_ok()).count();
    assert_eq!(made, 1, "{answers:?}");
    // The second answered finds the first's claim on the actor, before its worker starts.
    let refused = answers.iter().find_map(|answer| answer.as_ref().err());
    assert_eq!(
        refused.map(HostError::code),
        Some(ErrorCode::MatterFolderIsMatter)
    );
    assert_eq!(folder_matters(&host), 1);
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
}

#[test]
fn a_host_stopping_while_the_probe_waits_drops_its_answer() {
    let dir = TempDir::new("probe-stopped");
    let clock = clock();
    let (host, join, held) = start(dir.path(), &clock);
    let created = create(&host, 1);
    held.entered
        .recv_timeout(PATIENCE)
        .expect("the new folder probe was never entered");

    // Shutdown does not wait for the probe, and the create is answered as closed.
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
    let closed = created.join().unwrap().unwrap_err();
    assert_eq!(closed.code(), ErrorCode::Internal, "{closed:?}");

    // The probe answers with the actor gone: nothing is made, and nothing panics.
    held.release.send(()).unwrap();
    assert!(
        !held
            .answered
            .recv_timeout(PATIENCE)
            .expect("the probe never answered")
            .1
    );
    assert!(!new_folder(dir.path()).exists());
    block_on(async {
        let store = Store::open(StoreOptions::new(dir.path().join("data")))
            .await
            .unwrap();
        let rows = store.matters().await.unwrap();
        assert!(
            rows.iter().all(|row| row.kind != MatterKind::Folder),
            "a matter was stored"
        );
        store.close().await.unwrap();
    });
}

#[test]
fn a_locate_file_on_a_hung_share_leaves_the_host_answering() {
    let dir = TempDir::new("probe-locate");
    let clock = clock();
    let (host, join, held, locate) = start_both(dir.path(), &clock);
    let (file, paper) = matter_with_a_file(&host, &held);

    let asking = host.clone();
    let located =
        std::thread::spawn(move || block_on(asking.locate_file(Principal::LocalDesktop, file)));
    locate
        .entered
        .recv_timeout(PATIENCE)
        .expect("the locate probe was never entered");
    // The actor answers while the probe is held.
    assert_eq!(folder_matters(&host), 1);
    assert!(
        !located.is_finished(),
        "the locate answered with its probe held"
    );

    probe_wait_armed(&clock);
    clock.advance(PROBE_WAIT);
    let refused = located.join().unwrap().unwrap_err();
    assert_eq!(refused.code(), ErrorCode::MatterFolderMissing);
    let home = dir.path().to_string_lossy().into_owned();
    assert!(!refused.message().contains(&home), "{}", refused.message());

    // The late answer is dropped; a locate whose probe answers finds the file.
    locate.release.send(()).unwrap();
    assert_eq!(
        locate
            .answered
            .recv_timeout(PATIENCE)
            .expect("the probe never answered"),
        (true, true)
    );
    locate.release.send(()).unwrap();
    let found = block_on(host.locate_file(Principal::LocalDesktop, file)).unwrap();
    assert_eq!(found.path, paper);
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
}

/// A matter on the list, made by command 1, with a file in its folder: the file's ref.
///
/// The file is named under the matter's folder as the host stores it (its parent canonicalised
/// by the probe), as the desktop names a matter's papers: on macOS the temporary folder is
/// `/var/...`, a link to `/private/var/...`, and a ref under the other spelling is in no
/// matter's folder (`locate.rs`, `owner`), so its probe would never run.
fn matter_with_a_file(host: &HostHandle, held: &Held<(PathBuf, bool)>) -> (FileRefId, PathBuf) {
    let created = create(host, 1);
    held.entered
        .recv_timeout(PATIENCE)
        .expect("the new folder probe was never entered");
    held.release.send(()).unwrap();
    let CommandOutcome::MatterCreated { matter } = created.join().unwrap().unwrap() else {
        panic!("no matter made");
    };
    let folder = block_on(host.matter_folder(Principal::LocalDesktop, matter)).unwrap();
    let paper = folder.join("brief.txt");
    crate::write::write_file(&paper, b"x").unwrap();
    (block_on(host.register_file(paper.clone())).unwrap(), paper)
}

fn locate_on_a_thread(
    host: &HostHandle,
    file: FileRefId,
) -> JoinHandle<Result<LocatedFile, HostError>> {
    let host = host.clone();
    std::thread::spawn(move || block_on(host.locate_file(Principal::LocalDesktop, file)))
}

/// Starts shutdown on its own thread and returns once it waits for a probe.
fn stopping(host: &HostHandle, told: &mpsc::Receiver<()>) -> JoinHandle<Result<(), HostError>> {
    let stopper = host.clone();
    let stopped = std::thread::spawn(move || block_on(stopper.shutdown()));
    told.recv_timeout(PATIENCE)
        .expect("shutdown never waited for the probe");
    stopped
}

/// The matters the store holds that are not Scratch, once the host is gone.
fn stored_folder_matters(dir: &Path) -> usize {
    block_on(async {
        let store = Store::open(StoreOptions::new(dir.join("data")))
            .await
            .unwrap();
        let rows = store.matters().await.unwrap();
        store.close().await.unwrap();
        rows.iter()
            .filter(|row| row.kind == MatterKind::Folder)
            .count()
    })
}

#[test]
fn a_create_whose_probe_answers_while_the_host_stops_is_closed() {
    let dir = TempDir::new("probe-stopping");
    let clock = clock();
    let (told, hold) = mpsc::channel();
    let (host, join, held, _locate) = start_with(dir.path(), &clock, false, Some(told));
    let created = create(&host, 1);
    held.entered
        .recv_timeout(PATIENCE)
        .expect("the new folder probe was never entered");
    let stopped = stopping(&host, &hold);

    // The answer reaches an actor that is still there, stopping.
    held.release.send(()).unwrap();
    let closed = created.join().unwrap().unwrap_err();
    assert_eq!(closed.code(), ErrorCode::Internal, "{closed:?}");
    stopped.join().unwrap().unwrap();
    join.join().unwrap();
    assert!(!new_folder(dir.path()).exists());
    assert_eq!(stored_folder_matters(dir.path()), 0, "a matter was stored");
}

#[test]
fn a_locate_whose_probe_answers_while_the_host_stops_is_closed() {
    let dir = TempDir::new("probe-locate-stopping");
    let clock = clock();
    let (told, hold) = mpsc::channel();
    let (host, join, held, locate) = start_with(dir.path(), &clock, false, Some(told));
    let (file, _paper) = matter_with_a_file(&host, &held);
    let located = locate_on_a_thread(&host, file);
    locate
        .entered
        .recv_timeout(PATIENCE)
        .expect("the locate probe was never entered");
    let stopped = stopping(&host, &hold);

    locate.release.send(()).unwrap();
    let closed = located.join().unwrap().unwrap_err();
    assert_eq!(closed.code(), ErrorCode::Internal, "{closed:?}");
    stopped.join().unwrap().unwrap();
    join.join().unwrap();
}

#[test]
fn a_matter_forgotten_during_a_locate_is_no_matters_folder() {
    let dir = TempDir::new("probe-locate-forgotten");
    let clock = clock();
    let (host, join, held, locate) = start_both(dir.path(), &clock);
    let (file, _paper) = matter_with_a_file(&host, &held);
    let located = locate_on_a_thread(&host, file);
    locate
        .entered
        .recv_timeout(PATIENCE)
        .expect("the locate probe was never entered");

    let listed = block_on(host.query(Principal::LocalDesktop, HostQuery::ListMatters)).unwrap();
    let QueryResult::Matters(matters) = listed else {
        panic!("not matters");
    };
    let matter = matters
        .iter()
        .find(|matter| matter.kind == MatterKind::Folder)
        .unwrap()
        .matter;
    let forget = CommandEnvelope {
        command_id: CommandId::new(Uuid::from_u128(2)).unwrap(),
        command: HostCommand::ForgetMatter { matter },
    };
    block_on(host.dispatch(forget)).unwrap();

    // The probe finds the file in the folder, but the folder is no matter's now: as if asked
    // now, never `matter.folder_missing`, which offers to locate a folder.
    locate.release.send(()).unwrap();
    let refused = located.join().unwrap().unwrap_err();
    assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{refused:?}");
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
}

#[test]
fn a_create_racing_an_import_of_its_folder_makes_one_matter() {
    let dir = TempDir::new("probe-import-race");
    let clock = clock();
    // The probe looks before it is held: it finds no folder, as the import has not begun.
    let (host, join, held, _locate) = start_with(dir.path(), &clock, true, None);
    let created = create(&host, 1);
    held.entered
        .recv_timeout(PATIENCE)
        .expect("the new folder probe was never entered");

    // Meanwhile the lawyer makes the folder and imports it.
    crate::write::create_dir_all(&new_folder(dir.path())).unwrap();
    let folder = block_on(host.register_folder(new_folder(dir.path()))).unwrap();
    let importer = host.clone();
    let import = CommandEnvelope {
        command_id: CommandId::new(Uuid::from_u128(2)).unwrap(),
        command: HostCommand::CreateMatter {
            name: Title::new(NAME).unwrap(),
            source: MatterSource::ExistingFolder { folder },
            client: None,
            next_date: None,
        },
    };
    let imported = block_on(importer.dispatch(import));
    assert!(
        matches!(imported, Ok(CommandOutcome::MatterCreated { .. })),
        "{imported:?}"
    );

    // The create's probe answers "no folder" late: its create, re-checked on the actor against
    // the matters listed now, is refused, and no second matter has the folder.
    held.release.send(()).unwrap();
    assert!(
        !held
            .answered
            .recv_timeout(PATIENCE)
            .expect("the probe never answered")
            .1
    );
    let refused = created.join().unwrap().unwrap_err();
    assert_eq!(
        refused.code(),
        ErrorCode::MatterFolderIsMatter,
        "{refused:?}"
    );
    assert_eq!(folder_matters(&host), 1);
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
}
