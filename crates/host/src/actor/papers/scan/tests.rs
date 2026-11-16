//! The papers scan through a running host: a listing never waits for it, one `PapersChanged`
//! a batch, a superseded read publishes nothing, a batch stops at its time budget. The reads
//! run a stand-in (`revisions::hooks`) the test releases; never a sleep.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use clauseo_matters::Paper;
use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{
    CommandEnvelope, CommandId, CommandOutcome, HostCommand, HostEvent, HostQuery, MatterId,
    MatterPath, MatterSource, PaperSummary, Principal, QueryResult, Timestamp, Title,
};
use clauseo_settings::{Effective, Locations, SystemFiles};
use futures::executor::block_on;
use futures::{FutureExt, StreamExt};
use uuid::Uuid;

use super::{BATCH_TIME, read};
use crate::pipeline::tests::support::TempDir;
use crate::revisions::hooks;
use crate::{AppSubscription, Host, HostConfig, HostHandle, HostJoin, ManualClock};

/// The longest a test waits for the host or the stand-in: a bound on a hang, never a pause.
const PATIENCE: Duration = Duration::from_secs(30);

/// A running host with one matter of `files`, the stand-in reads gated by the test.
struct Running {
    _dir: TempDir,
    folder: PathBuf,
    host: HostHandle,
    join: HostJoin,
    app: AppSubscription,
    matter: MatterId,
    /// Each read the stand-in starts, by file name.
    started: mpsc::Receiver<String>,
    /// One message lets one read finish.
    release: mpsc::Sender<()>,
}

impl Running {
    /// `files` in a new matter; a read answers that a file holds marks when its name starts
    /// with `marked`, once the test releases it.
    fn start(test: &str, files: &[(&str, &str)]) -> Self {
        let dir = TempDir::new(test);
        let data = dir.path().join("data");
        crate::write::create_dir_all(&data).unwrap();
        let effective = Effective::load(&Locations::under_home(dir.path()), &SystemFiles, &[]);
        let launcher = PassthroughLauncher::new(SpawnOptions::new().home(dir.path().join("home")));
        let clock = ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
        let config = HostConfig::new(effective, Arc::new(launcher))
            .with_clock(Arc::new(clock))
            .with_ids(Box::new(crate::SequentialIds::with_run(1)));
        let (host, join) = Host::start(config).unwrap();
        let app = block_on(host.subscribe_app(Principal::LocalDesktop)).unwrap();
        let folder = dir.path().join("work").join("m");
        crate::write::create_dir_all(&folder).unwrap();
        for (file, text) in files {
            crate::write::write_file(&folder.join(file), text.as_bytes()).unwrap();
        }
        let (started_tx, started) = mpsc::channel();
        let (release, release_rx) = mpsc::channel::<()>();
        let gate = Mutex::new((started_tx, release_rx));
        hooks::set(
            &folder,
            Arc::new(move |path: &Path| {
                let name = path.file_name()?.to_str()?.to_owned();
                let gate = gate.lock().unwrap();
                let _gone = gate.0.send(name.clone());
                gate.1.recv_timeout(PATIENCE).ok()?;
                Some(name.starts_with("marked"))
            }),
        );
        let id = block_on(host.register_folder(folder.clone())).unwrap();
        let create = HostCommand::CreateMatter {
            name: Title::new("M").unwrap(),
            source: MatterSource::ExistingFolder { folder: id },
            client: None,
            next_date: None,
        };
        let command = CommandEnvelope {
            command_id: CommandId::new(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001))
                .unwrap(),
            command: create,
        };
        let CommandOutcome::MatterCreated { matter } = block_on(host.dispatch(command)).unwrap()
        else {
            panic!("no matter");
        };
        Self {
            _dir: dir,
            folder,
            host,
            join,
            app,
            matter,
            started,
            release,
        }
    }

    /// The papers as `ListPapers` answers now.
    fn list(&self) -> Vec<PaperSummary> {
        let query = HostQuery::ListPapers {
            matter: self.matter,
        };
        let answer = block_on(self.host.query(Principal::LocalDesktop, query)).unwrap();
        let QueryResult::Papers { papers, .. } = answer else {
            panic!("ListPapers answered another result");
        };
        papers
    }

    /// Whether `name` is listed with Tracked changes.
    fn tracked(&self, name: &str) -> bool {
        let papers = self.list();
        let paper = papers.iter().find(|paper| paper.path.as_str() == name);
        paper.unwrap_or_else(|| panic!("{name} not listed")).tracked
    }

    /// Waits for the stand-in to start reading a file; its name.
    fn read_started(&self) -> String {
        self.started.recv_timeout(PATIENCE).unwrap()
    }

    /// Waits for the next `PapersChanged`, skipping the other app events.
    fn papers_changed(&mut self) -> MatterId {
        loop {
            let next = block_on(self.app.next()).expect("the app stream ended");
            if let HostEvent::PapersChanged { matter } = next.decode().unwrap() {
                return matter;
            }
        }
    }

    /// The `PapersChanged` events already in the app channel, without waiting.
    fn ready_changes(&mut self) -> usize {
        let mut changes = 0;
        while let Some(Some(item)) = self.app.next().now_or_never() {
            if let HostEvent::PapersChanged { .. } = item.decode().unwrap() {
                changes += 1;
            }
        }
        changes
    }

    fn shutdown(self) {
        block_on(self.host.shutdown()).unwrap();
        self.join.join().unwrap();
    }
}

#[test]
fn a_listing_answers_before_the_scan_and_papers_changed_brings_tracked() {
    let files = [
        ("marked.docx", "a"),
        ("clean.docx", "b"),
        ("notes.txt", "c"),
    ];
    let mut host = Running::start("papers_scan_first", &files);
    // The first listing answers while the stand-in holds the first read.
    assert!(!host.tracked("marked.docx"));
    // Papers are read in the listing's order, one at a time.
    assert_eq!(host.read_started(), "clean.docx");
    assert!(
        !host.tracked("marked.docx"),
        "a second listing waits for nothing either"
    );
    host.release.send(()).unwrap();
    assert_eq!(host.read_started(), "marked.docx");
    host.release.send(()).unwrap();
    assert_eq!(host.papers_changed(), host.matter);
    assert!(host.tracked("marked.docx"));
    assert!(!host.tracked("clean.docx"));
    // Listed again, nothing is read again and nothing more is published.
    assert_eq!(host.ready_changes(), 0);
    assert!(host.started.try_recv().is_err());
    host.shutdown();
}

#[test]
fn a_batch_of_many_files_publishes_one_papers_changed() {
    let names: Vec<String> = (0..40).map(|n| format!("marked {n:02}.docx")).collect();
    let files: Vec<(&str, &str)> = names.iter().map(|name| (name.as_str(), "x")).collect();
    let mut host = Running::start("papers_scan_one_event", &files);
    assert!(host.list().iter().all(|paper| !paper.tracked));
    for _ in 0..40 {
        host.read_started();
        host.release.send(()).unwrap();
    }
    assert_eq!(host.papers_changed(), host.matter);
    assert!(host.list().iter().all(|paper| paper.tracked));
    // The listing answered after the batch was taken in: any second event would be here.
    assert_eq!(host.ready_changes(), 0);
    host.shutdown();
}

#[test]
fn a_read_of_a_file_changed_since_publishes_nothing_and_the_new_one_is_read() {
    let mut host = Running::start("papers_scan_superseded", &[("marked.docx", "old")]);
    assert!(!host.tracked("marked.docx"));
    assert_eq!(host.read_started(), "marked.docx");
    // Saved again while the read runs: the next listing has another size.
    crate::write::write_file(&host.folder.join("marked.docx"), b"a longer text").unwrap();
    assert!(!host.tracked("marked.docx"));
    host.release.send(()).unwrap();
    // The stale read is dropped unpublished; the file is read again as it is now. One scan at
    // a time, so this read starts only after the stale batch was taken in.
    assert_eq!(host.read_started(), "marked.docx");
    host.release.send(()).unwrap();
    assert_eq!(host.papers_changed(), host.matter);
    assert!(host.tracked("marked.docx"));
    assert_eq!(host.ready_changes(), 0, "the stale batch published nothing");
    host.shutdown();
}

#[test]
fn a_batch_starts_no_file_after_its_time_budget() {
    let dir = TempDir::new("papers_scan_budget");
    let clock = ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
    let ticking = clock.clone();
    hooks::set(
        dir.path(),
        Arc::new(move |_path: &Path| {
            ticking.advance(BATCH_TIME);
            Some(true)
        }),
    );
    let paper = |name: &str| Paper {
        path: MatterPath::new(name).unwrap(),
        size: 1,
        modified: Timestamp::new(0).unwrap(),
    };
    let papers = vec![paper("a.docx"), paper("b.docx"), paper("c.docx")];
    let matter = MatterId::new(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0009)).unwrap();
    let batch = read(matter, dir.path().to_path_buf(), papers, &clock);
    let read: Vec<&str> = batch
        .read
        .iter()
        .map(|(paper, _)| paper.path.as_str())
        .collect();
    assert_eq!(read, ["a.docx"]);
}
