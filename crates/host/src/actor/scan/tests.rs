//! Folder scans through a running host: the throttle on the manual clock, one scan at a time,
//! cancel, the last event, shutdown, refusals, and the totals against an import's. Scans that
//! need stepping run a stand-in walk the test drives ([`Harness`]); never a sleep.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use clauseo_matters::{FolderScan, FolderTotals, ScanProgress};
use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{
    CommandEnvelope, CommandId, CommandOutcome, DomainEvent, ErrorCode, FolderPlace, FolderRefId,
    FolderScanned, HostCommand, HostError, HostEvent, MatterPath, MatterSource, Principal,
    StreamId, Timestamp, Title,
};
use clauseo_settings::{Effective, Locations, SystemFiles};
use futures::executor::block_on;
use futures::{FutureExt, StreamExt};
use uuid::Uuid;

use super::{Offer, THROTTLE, Throttle, Walk};
use crate::pipeline::tests::support::TempDir;
use crate::{AppSubscription, Host, HostConfig, HostHandle, HostJoin, ManualClock};

#[test]
fn the_throttle_sends_once_per_window_and_the_timer_sends_what_it_held() {
    let ms = Duration::from_millis;
    let mut throttle = Throttle::default();
    assert_eq!(throttle.offer(ms(0)), Offer::Send);
    assert_eq!(throttle.offer(ms(10)), Offer::Arm(ms(240)));
    assert_eq!(throttle.offer(ms(20)), Offer::Hold);
    assert!(throttle.tick(ms(250)));
    // Nothing held: the next tick sends nothing, and progress a window later goes at once.
    assert!(!throttle.tick(ms(400)));
    assert_eq!(throttle.offer(ms(500)), Offer::Send);
}

/// What a test tells one stand-in walk.
enum Step {
    /// Report `files` files of 10 bytes each, then check the cancel flag.
    Files(u64),
    /// Check the cancel flag only.
    Check,
    /// Finish with the last counts.
    End,
}

/// One call of the stand-in walk: the folder, its steps and its acknowledgements (each says
/// whether the cancel flag was set).
struct Call {
    folder: PathBuf,
    steps: mpsc::Sender<Step>,
    acks: mpsc::Receiver<bool>,
}

impl Call {
    fn step(&self, step: Step) -> bool {
        self.steps.send(step).unwrap();
        self.acks.recv().unwrap()
    }
}

/// The calls of the stand-in walk, in order.
struct Harness {
    calls: mpsc::Receiver<Call>,
}

impl Harness {
    /// A walk that hands each call to the test, and the harness that receives them.
    fn new() -> (Walk, Self) {
        let (send, calls) = mpsc::channel::<Call>();
        let send = Mutex::new(send);
        let walk: Walk = Arc::new(move |folder, _refused, _settings, progress, cancel| {
            let (steps, step_rx) = mpsc::channel();
            let (ack, acks) = mpsc::channel();
            let call = Call {
                folder: folder.to_path_buf(),
                steps,
                acks,
            };
            send.lock().unwrap().send(call).unwrap();
            let file = MatterPath::new("a.docx").unwrap();
            let mut files = 0;
            // A test that drops its call ends the walk.
            while let Ok(step) = step_rx.recv() {
                match step {
                    Step::Files(n) => {
                        files = n;
                        progress(ScanProgress {
                            files,
                            bytes: files * 10,
                            cloud_only: 0,
                            now_reading: &file,
                        });
                    }
                    Step::Check => {}
                    Step::End => break,
                }
                let cancelled = cancel.load(Ordering::Relaxed);
                let _gone = ack.send(cancelled);
                if cancelled {
                    return Ok(FolderScan::Cancelled);
                }
            }
            Ok(FolderScan::Done(FolderTotals {
                files,
                bytes: files * 10,
                cloud_only: 0,
            }))
        });
        (walk, Self { calls })
    }

    fn next(&self) -> Call {
        self.calls.recv_timeout(Duration::from_secs(30)).unwrap()
    }
}

/// A running host on its own folder, with the manual clock.
struct Running {
    dir: TempDir,
    host: HostHandle,
    join: HostJoin,
    clock: ManualClock,
    app: AppSubscription,
}

impl Running {
    fn start(test: &str, walk: Option<Walk>) -> Self {
        let dir = TempDir::new(test);
        // The data folder beside the scanned folders, never around them.
        let data = dir.path().join("data");
        crate::write::create_dir_all(&data).unwrap();
        let effective = Effective::load(&Locations::under_home(dir.path()), &SystemFiles, &[]);
        let launcher = PassthroughLauncher::new(SpawnOptions::new().home(dir.path().join("home")));
        let clock = ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
        let mut config = HostConfig::new(effective, Arc::new(launcher))
            .with_clock(Arc::new(clock.clone()))
            .with_ids(Box::new(crate::SequentialIds::with_run(1)));
        if let Some(walk) = walk {
            config = config.with_scan_walk(walk);
        }
        let (host, join) = Host::start(config).unwrap();
        let app = block_on(host.subscribe_app(Principal::LocalDesktop)).unwrap();
        Self {
            dir,
            host,
            join,
            clock,
            app,
        }
    }

    /// A folder under the test's folder holding `files`, and its ref.
    fn folder(&self, name: &str, files: &[(&str, &str)]) -> (PathBuf, FolderRefId) {
        let folder = self.dir.path().join("work").join(name);
        crate::write::create_dir_all(&folder).unwrap();
        for (file, text) in files {
            crate::write::write_file(&folder.join(file), text.as_bytes()).unwrap();
        }
        let id = block_on(self.host.register_folder(folder.clone())).unwrap();
        (folder, id)
    }

    /// Dispatches `command` on its own thread, since a scan answers only when it ends.
    fn send(&self, n: u128, command: HostCommand) -> JoinHandle<Result<CommandOutcome, HostError>> {
        let host = self.host.clone();
        std::thread::spawn(move || block_on(host.dispatch(envelope(n, command))))
    }

    fn dispatch(&self, n: u128, command: HostCommand) -> Result<CommandOutcome, HostError> {
        block_on(self.host.dispatch(envelope(n, command)))
    }

    /// The next `FolderScanned`, skipping the other app events.
    fn next_scan(&mut self) -> FolderScanned {
        next_scan_of(&mut self.app)
    }

    /// The scan events already in the app channel, without waiting.
    fn ready_scans(&mut self) -> Vec<FolderScanned> {
        let mut events = Vec::new();
        while let Some(Some(item)) = self.app.next().now_or_never() {
            if let HostEvent::FolderScanned(event) = item.decode().unwrap() {
                events.push(event);
            }
        }
        events
    }

    /// Waits until the actor arms the throttle's timer.
    fn wait_throttle_armed(&self) {
        let clock = self.clock.clone();
        block_on(async move {
            for _ in 0..64 {
                if clock.next_armed().await == THROTTLE {
                    return;
                }
            }
            panic!("the throttle's timer was never armed");
        });
    }

    fn shutdown(self) {
        block_on(self.host.shutdown()).unwrap();
        self.join.join().unwrap();
    }
}

/// The next `FolderScanned` on `app`, skipping the other app events.
fn next_scan_of(app: &mut AppSubscription) -> FolderScanned {
    loop {
        match block_on(app.next()).unwrap().decode().unwrap() {
            HostEvent::FolderScanned(event) => return event,
            HostEvent::SettingsChanged(_)
            | HostEvent::MattersChanged(_)
            | HostEvent::ThreadsChanged(_)
            | HostEvent::BrowserInstall { .. } => {}
            other @ (HostEvent::Patches(_)
            | HostEvent::Events(_)
            | HostEvent::CaughtUp { .. }
            | HostEvent::AccountLimitsChanged(_)
            | HostEvent::LimitsViewChanged(_)) => panic!("unexpected {other:?}"),
        }
    }
}

fn envelope(n: u128, command: HostCommand) -> CommandEnvelope {
    CommandEnvelope {
        command_id: CommandId::new(Uuid::from_u128(
            0x0190_0000_0000_7000_8000_0000_0000_0000 + n,
        ))
        .unwrap(),
        command,
    }
}

fn scan(folder: FolderRefId) -> HostCommand {
    HostCommand::ScanFolder { folder }
}

fn done(joined: JoinHandle<Result<CommandOutcome, HostError>>) {
    assert_eq!(joined.join().unwrap().unwrap(), CommandOutcome::Done);
}

#[test]
fn progress_is_throttled_to_one_event_per_window_on_the_host_clock() {
    let (walk, harness) = Harness::new();
    let mut host = Running::start("scan-throttle", Some(walk));
    let (_, folder) = host.folder("m", &[]);
    let running = host.send(1, scan(folder));
    let call = harness.next();

    // The place goes out at once.
    let first = host.next_scan();
    assert_eq!((first.files, first.place.clone()), (0, FolderPlace::Local));
    assert!(!first.done);
    // Ten files inside one window: held, not sent.
    for n in 1..=10 {
        assert!(!call.step(Step::Files(n)));
    }
    host.wait_throttle_armed();
    assert_eq!(host.ready_scans(), []);
    // The window passes: the timer sends the latest counts.
    host.clock.advance(THROTTLE);
    let held = host.next_scan();
    assert_eq!((held.files, held.bytes.get(), held.done), (10, 100, false));
    assert_eq!(held.now_reading.unwrap().as_str(), "a.docx");
    // The end goes out inside the next window too.
    call.steps.send(Step::End).unwrap();
    let last = host.next_scan();
    assert_eq!(
        (last.files, last.bytes.get(), last.done, last.cancelled),
        (10, 100, true, false)
    );
    assert_eq!(last.now_reading, None);
    done(running);
    host.shutdown();
}

/// A scan of another folder cancels the first, whose cancelled end is still published.
#[test]
fn a_second_scan_cancels_the_first() {
    let (walk, harness) = Harness::new();
    let mut host = Running::start("scan-second", Some(walk));
    let (first_path, first) = host.folder("a", &[]);
    let (second_path, second) = host.folder("b", &[]);
    let one = host.send(1, scan(first));
    let first_call = harness.next();
    assert_eq!(first_call.folder, first_path);
    assert_eq!(host.next_scan().folder, first);

    let two = host.send(2, scan(second));
    let second_call = harness.next();
    assert_eq!(second_call.folder, second_path);
    // The first scan's flag was set before the second started.
    assert!(first_call.step(Step::Check));
    let mut ended_first = None;
    let mut placed_second = false;
    while ended_first.is_none() || !placed_second {
        let event = host.next_scan();
        if event.folder == first {
            ended_first = Some(event);
        } else {
            placed_second = true;
        }
    }
    let ended_first = ended_first.unwrap();
    assert!(ended_first.done && ended_first.cancelled);
    done(one);

    second_call.steps.send(Step::End).unwrap();
    let last = host.next_scan();
    assert_eq!(
        (last.folder, last.done, last.cancelled),
        (second, true, false)
    );
    done(two);
    host.shutdown();
}

/// A scan of a folder superseded by a newer scan of the same folder ends without a word, even
/// when it ends after the newer one (stuck on a hung stat): the folder's last event stays the
/// newer scan's `done`, live and in a new subscriber's catch-up, and the older command is still
/// answered.
#[test]
fn a_superseded_scan_of_the_same_folder_ends_without_an_event() {
    let (walk, harness) = Harness::new();
    let mut host = Running::start("scan-same-folder", Some(walk));
    let (_, folder) = host.folder("m", &[]);
    let one = host.send(1, scan(folder));
    let first_call = harness.next();
    assert_eq!(host.next_scan().folder, folder);

    let two = host.send(2, scan(folder));
    let second_call = harness.next();
    let placed = host.next_scan();
    assert!(!placed.done, "{placed:?}");
    second_call.steps.send(Step::End).unwrap();
    let last = host.next_scan();
    assert_eq!(
        (last.folder, last.done, last.cancelled),
        (folder, true, false)
    );
    done(two);

    // The first walk returns only now, cancelled.
    assert!(first_call.step(Step::Check));
    done(one);
    assert_eq!(
        host.ready_scans(),
        [],
        "a superseded scan published its end"
    );

    // A window that subscribes now catches up on the newer scan's end.
    let mut late = block_on(host.host.subscribe_app(Principal::LocalDesktop)).unwrap();
    let caught = next_scan_of(&mut late);
    assert_eq!(caught, last);
    host.shutdown();
}

#[test]
fn cancel_scan_ends_the_walk_with_cancelled() {
    let (walk, harness) = Harness::new();
    let mut host = Running::start("scan-cancel", Some(walk));
    let (_, folder) = host.folder("m", &[]);
    let running = host.send(1, scan(folder));
    let call = harness.next();
    host.next_scan();
    assert!(!call.step(Step::Files(1)));

    let cancel = HostCommand::CancelScan { folder };
    assert_eq!(host.dispatch(2, cancel).unwrap(), CommandOutcome::Done);
    assert!(call.step(Step::Check));
    let last = host.next_scan();
    assert_eq!(
        (last.files, last.done, last.cancelled),
        (1, true, true),
        "{last:?}"
    );
    done(running);
    host.shutdown();
}

#[test]
fn cancel_scan_of_a_folder_with_no_scan_starts_nothing() {
    let (walk, harness) = Harness::new();
    let mut host = Running::start("scan-cancel-unknown", Some(walk));
    let unknown =
        FolderRefId::new(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00ff)).unwrap();
    let cancel = HostCommand::CancelScan { folder: unknown };
    assert_eq!(host.dispatch(1, cancel).unwrap(), CommandOutcome::Done);
    // A round trip after it, so anything it had started would have shown by now.
    let (_, folder) = host.folder("m", &[]);
    let cancel = HostCommand::CancelScan { folder };
    assert_eq!(host.dispatch(2, cancel).unwrap(), CommandOutcome::Done);
    assert!(harness.calls.try_recv().is_err());
    assert_eq!(host.ready_scans(), []);
    host.shutdown();
}

#[test]
fn the_last_event_has_the_totals_of_an_import_of_the_same_folder() {
    let mut host = Running::start("scan-totals", None);
    let files = [("a.txt", "one"), ("b.docx", "twelve"), ("~$b.docx", "lock")];
    let (_, folder) = host.folder("m", &files);
    // The clock never moves, so only the first event and the last go out.
    assert_eq!(
        host.dispatch(1, scan(folder)).unwrap(),
        CommandOutcome::Done
    );
    let mut events = vec![host.next_scan()];
    while !events.last().unwrap().done {
        events.push(host.next_scan());
    }
    assert!(events.len() <= 3, "{events:?}");
    let last = events.last().unwrap().clone();
    assert!(!last.cancelled);

    let create = HostCommand::CreateMatter {
        name: Title::new("M").unwrap(),
        source: MatterSource::ExistingFolder { folder },
        client: None,
        next_date: None,
    };
    let CommandOutcome::MatterCreated { matter } = host.dispatch(2, create).unwrap() else {
        panic!("no matter");
    };
    let summary = import_summary(&host.host, matter);
    assert_eq!((last.files, last.bytes), (summary.files, summary.bytes));
    assert_eq!((last.files, last.bytes.get()), (2, 9));
    assert_eq!(last.cloud_only, summary.cloud_only);
    host.shutdown();
}

/// The import summary of `matter`'s `matter_created`.
fn import_summary(
    host: &HostHandle,
    matter: clauseo_protocol::MatterId,
) -> clauseo_protocol::ImportSummary {
    let stream = StreamId::Matter(matter);
    let mut events =
        block_on(host.subscribe_events(Principal::LocalDesktop, stream, None)).unwrap();
    loop {
        match block_on(events.next()).unwrap() {
            HostEvent::Events(envelopes) => {
                for envelope in envelopes {
                    if let DomainEvent::MatterCreated(created) = &envelope.event {
                        return created.import().unwrap();
                    }
                }
            }
            other @ (HostEvent::Patches(_)
            | HostEvent::CaughtUp { .. }
            | HostEvent::AccountLimitsChanged(_)
            | HostEvent::LimitsViewChanged(_)
            | HostEvent::SettingsChanged(_)
            | HostEvent::MattersChanged(_)
            | HostEvent::ThreadsChanged(_)
            | HostEvent::FolderScanned(_)
            | HostEvent::BrowserInstall { .. }) => panic!("no matter_created before {other:?}"),
        }
    }
}

#[test]
fn shutdown_during_a_scan_sets_its_flag_and_does_not_wait_for_it() {
    let (walk, harness) = Harness::new();
    let host = Running::start("scan-shutdown", Some(walk));
    let (_, folder) = host.folder("m", &[]);
    let running = host.send(1, scan(folder));
    let call = harness.next();
    // The walk is stuck in its step: shutdown returns all the same.
    host.shutdown();
    assert!(call.step(Step::Check));
    // The host stopped before the scan answered.
    assert!(running.join().unwrap().is_err());
}

#[test]
fn an_unknown_folder_ref_and_a_refused_folder_are_refused() {
    let mut host = Running::start("scan-refused", None);
    let unknown =
        FolderRefId::new(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00fe)).unwrap();
    let error = host.dispatch(1, scan(unknown)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error}");
    assert_eq!(host.ready_scans(), []);

    // The data folder: import refuses it with everything inside it.
    let data = host.dir.path().join("data");
    let refused = block_on(host.host.register_folder(data)).unwrap();
    let error = host.dispatch(2, scan(refused)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterFolderRefused, "{error}");
    // The window still hears the scan ended.
    let mut last = host.next_scan();
    while !last.done {
        last = host.next_scan();
    }
    assert_eq!((last.folder, last.cancelled), (refused, false));
    host.shutdown();
}
