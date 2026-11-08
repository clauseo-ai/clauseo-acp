//! The client over a fake [`Watch`]: routing, `dropped`, the cap, unknown access, the fallback
//! states, the second request, the restart rate and the server check. The fake's clock moves
//! only when the client sleeps or reads, so no test waits out a real minute.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};

use super::*;
use crate::Denial;
use crate::test_support::TempDir;
use std::path::PathBuf;

const POOL_A: &str = "S-1-5-21-1-2-3-1101";
const POOL_B: &str = "S-1-5-21-1-2-3-1102";
const NAME: &str = r"\\.\pipe\clauseo-sandbox-watch-0123456789abcdef0123456789abcdef";

/// What the fake does, set by each test.
struct Script {
    request: Result<(), StartError>,
    /// The published name, and from when it is there.
    name: Result<Option<String>, String>,
    name_from: Duration,
    /// What each connect gives, in order; `Absent` once empty.
    connects: VecDeque<Result<Receiver<Vec<u8>>, ConnectError>>,
    last_exit: Option<u32>,
    /// The IT off switch.
    off: bool,
    /// `would_deny`'s answers for a read, by path.
    deny: BTreeMap<PathBuf, Option<bool>>,
}

impl Default for Script {
    fn default() -> Script {
        Script {
            request: Ok(()),
            name: Ok(Some(NAME.to_owned())),
            name_from: Duration::ZERO,
            connects: VecDeque::new(),
            last_exit: None,
            off: false,
            deny: BTreeMap::new(),
        }
    }
}

#[derive(Default)]
struct Inner {
    ms: AtomicU64,
    script: Mutex<Script>,
    requests: Mutex<Vec<Duration>>,
    connects: Mutex<Vec<String>>,
    logs: Mutex<Vec<(bool, String)>>,
    /// Every path `would_deny` was asked about.
    asked: Mutex<Vec<PathBuf>>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Inner>);

impl Fake {
    fn script(&self) -> MutexGuard<'_, Script> {
        self.0.script.lock().unwrap()
    }

    /// A pipe the next connect gives; the sender feeds it, and dropping it ends it.
    fn pipe(&self) -> Sender<Vec<u8>> {
        let (send, receive) = channel();
        self.script().connects.push_back(Ok(receive));
        send
    }

    fn requests(&self) -> Vec<Duration> {
        self.0.requests.lock().unwrap().clone()
    }

    fn errors(&self) -> Vec<String> {
        let logs = self.0.logs.lock().unwrap();
        logs.iter()
            .filter(|(error, _)| *error)
            .map(|(_, line)| line.clone())
            .collect()
    }

    fn advance(&self, by: Duration) {
        let ms = u64::try_from(by.as_millis()).unwrap();
        self.0.ms.fetch_add(ms, Ordering::SeqCst);
    }
}

struct FakePipe {
    receive: Receiver<Vec<u8>>,
    fake: Fake,
    pending: VecDeque<u8>,
}

impl Pipe for FakePipe {
    fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<Option<usize>> {
        if self.pending.is_empty() {
            match self.receive.recv_timeout(Duration::from_millis(2)) {
                Ok(bytes) => self.pending.extend(bytes),
                Err(RecvTimeoutError::Timeout) => {
                    self.fake.advance(wait);
                    return Ok(None);
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(Some(0)),
            }
        }
        let n = buf.len().min(self.pending.len());
        for (slot, byte) in buf.iter_mut().zip(self.pending.drain(..n)) {
            *slot = byte;
        }
        Ok(Some(n))
    }
}

impl Watch for Fake {
    type Pipe = FakePipe;

    fn request(&self) -> Result<(), StartError> {
        self.0.requests.lock().unwrap().push(self.now());
        self.script().request.clone()
    }

    fn pipe_name(&self) -> Result<Option<String>, String> {
        let now = self.now();
        let script = self.script();
        if now < script.name_from {
            return Ok(None);
        }
        script.name.clone()
    }

    fn connect(&self, name: &str) -> Result<FakePipe, ConnectError> {
        self.0.connects.lock().unwrap().push(name.to_owned());
        let next = self.script().connects.pop_front();
        match next {
            Some(Ok(receive)) => Ok(FakePipe {
                receive,
                fake: self.clone(),
                pending: VecDeque::new(),
            }),
            Some(Err(error)) => Err(error),
            None => Err(ConnectError::Absent),
        }
    }

    fn turned_off(&self) -> bool {
        self.script().off
    }

    fn last_exit(&self) -> Option<u32> {
        self.script().last_exit
    }

    fn would_deny(&self, _pool_user: &Sid, path: &Path, access: Access) -> Option<bool> {
        assert_eq!(access, Access::Read, "the client asks about a read");
        self.0.asked.lock().unwrap().push(path.to_owned());
        self.script().deny.get(path).copied().flatten()
    }

    fn now(&self) -> Duration {
        Duration::from_millis(self.0.ms.load(Ordering::SeqCst))
    }

    fn sleep(&self, wait: Duration) {
        self.advance(wait);
        std::thread::yield_now();
    }

    fn log(&self, error: bool, line: &str) {
        self.0.logs.lock().unwrap().push((error, line.to_owned()));
    }
}

/// A sink that keeps what it gets.
#[derive(Default)]
struct Keep(Mutex<Vec<Denial>>);

impl Keep {
    fn got(&self) -> Vec<Denial> {
        self.0.lock().unwrap().clone()
    }
}

impl DenialSink for Keep {
    fn denied(&self, d: Denial) {
        self.0.lock().unwrap().push(d);
    }
}

/// A clock the test moves by hand, for a launch's cap.
#[derive(Default)]
struct HandClock(AtomicU64);

impl Clock for Arc<HandClock> {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
}

struct Launch {
    keep: Arc<Keep>,
    clock: Arc<HandClock>,
    sink: Arc<dyn Capped>,
}

fn launch() -> Launch {
    let keep = Arc::new(Keep::default());
    let clock = Arc::new(HandClock::default());
    let inner = Arc::clone(&keep) as Arc<dyn DenialSink>;
    let sink: Arc<dyn Capped> = Arc::new(CappedSink::with_clock(inner, Arc::clone(&clock)));
    Launch { keep, clock, sink }
}

fn policy(dir: &TempDir) -> Arc<Policy> {
    dir.dir("m");
    dir.dir("s");
    let policy = Policy::builder(dir.path().join("m"), dir.path().join("s"))
        .home(dir.path().join("home"))
        .build()
        .unwrap();
    Arc::new(policy)
}

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

fn denied(sid: &str, path: &str, access: &str) -> Vec<u8> {
    line(sid, path, access, false)
}

/// A refusal line that always carries `net`, as the watcher sends it.
fn line(sid: &str, path: &str, access: &str, net: bool) -> Vec<u8> {
    let line = serde_json::json!({
        "v": 1, "sid": sid, "pid": 7, "path": path, "access": access, "net": net,
    });
    let mut bytes = serde_json::to_vec(&line).unwrap();
    bytes.push(b'\n');
    bytes
}

fn path_denial(policy: &Policy, path: &str, access: Access) -> Denial {
    let path = PathBuf::from(path);
    let class = policy.classify(&path, access);
    Denial::Path {
        path,
        access,
        class,
    }
}

/// Waits (real time, at most 5 s) until `done` holds.
fn eventually(what: &str, done: impl Fn() -> bool) {
    let (_keep, pause) = channel::<()>();
    for _ in 0..1000 {
        if done() {
            return;
        }
        // Nothing is ever sent: the timeout is the pause.
        pause.recv_timeout(Duration::from_millis(5)).unwrap_err();
    }
    panic!("never: {what}");
}

#[test]
fn lines_split_at_newlines_across_chunks_and_an_overlong_one_is_skipped() {
    let mut lines = Lines::default();
    assert_eq!(lines.push(b"ab"), Vec::<Vec<u8>>::new());
    assert_eq!(
        lines.push(b"c\nd\n\ne"),
        [b"abc".to_vec(), b"d".to_vec(), Vec::new()]
    );
    let long = vec![b'x'; LINE_CAP + 1];
    assert_eq!(lines.push(&long), Vec::<Vec<u8>>::new());
    assert_eq!(lines.push(b"tail\nnext\n"), [b"next".to_vec()]);
}

#[test]
fn refusals_go_to_the_launch_of_their_pool_user_and_unknown_ones_are_dropped_quietly() {
    let fake = Fake::default();
    let pipe = fake.pipe();
    let hub = Hub::new(fake.clone());
    let dir = TempDir::new("watch-client-route");
    let policy = policy(&dir);
    let (a, b) = (launch(), launch());
    let _a = hub.register(sid(POOL_A), Arc::clone(&policy), Arc::clone(&a.sink));
    assert_eq!(hub.status(), Status::Live);
    let _b = hub.register(sid(POOL_B), Arc::clone(&policy), Arc::clone(&b.sink));

    pipe.send(denied(POOL_B, r"C:\b.txt", "write")).unwrap();
    pipe.send(denied(POOL_A, r"C:\a.txt", "read")).unwrap();
    // Another SID, another version, not JSON: nothing.
    pipe.send(denied("S-1-5-21-9-9-9-1", r"C:\x", "read"))
        .unwrap();
    pipe.send(b"{\"v\":2,\"dropped\":3}\nnot json\n".to_vec())
        .unwrap();
    pipe.send(denied(POOL_A, r"C:\last.txt", "write")).unwrap();
    eventually("both launches got theirs", || {
        a.keep.got().len() == 2 && !b.keep.got().is_empty()
    });
    assert_eq!(
        a.keep.got(),
        [
            path_denial(&policy, r"C:\a.txt", Access::Read),
            path_denial(&policy, r"C:\last.txt", Access::Write),
        ]
    );
    assert_eq!(
        b.keep.got(),
        [path_denial(&policy, r"C:\b.txt", Access::Write)]
    );
    assert_eq!(fake.0.connects.lock().unwrap().as_slice(), [NAME]);
}

#[test]
fn the_newest_registration_of_a_pool_user_gets_its_refusals() {
    let fake = Fake::default();
    let pipe = fake.pipe();
    let hub = Hub::new(fake);
    let dir = TempDir::new("watch-client-newest");
    let policy = policy(&dir);
    let (old, new) = (launch(), launch());
    let _old = hub.register(sid(POOL_A), Arc::clone(&policy), Arc::clone(&old.sink));
    let _new = hub.register(sid(POOL_A), Arc::clone(&policy), Arc::clone(&new.sink));
    pipe.send(denied(POOL_A, r"C:\a.txt", "write")).unwrap();
    eventually("the newest got it", || new.keep.got().len() == 1);
    assert_eq!(old.keep.got(), []);
}

#[test]
fn dropped_goes_to_the_last_refusals_launch_or_to_every_launch_before_one() {
    let fake = Fake::default();
    let pipe = fake.pipe();
    let hub = Hub::new(fake);
    let dir = TempDir::new("watch-client-dropped");
    let policy = policy(&dir);
    let (a, b) = (launch(), launch());
    let _a = hub.register(sid(POOL_A), Arc::clone(&policy), Arc::clone(&a.sink));
    let _b = hub.register(sid(POOL_B), Arc::clone(&policy), Arc::clone(&b.sink));
    pipe.send(b"{\"v\":1,\"dropped\":4}\n".to_vec()).unwrap();
    eventually("both got the first count", || {
        a.keep.got().len() == 1 && b.keep.got().len() == 1
    });
    assert_eq!(a.keep.got(), [Denial::Dropped { count: 4 }]);
    pipe.send(denied(POOL_B, r"C:\b", "write")).unwrap();
    pipe.send(b"{\"v\":1,\"dropped\":0}\n{\"v\":1,\"dropped\":9}\n".to_vec())
        .unwrap();
    eventually("b got the second count", || b.keep.got().len() == 3);
    assert_eq!(b.keep.got().last(), Some(&Denial::Dropped { count: 9 }));
    assert_eq!(a.keep.got().len(), 1);
}

#[test]
fn a_launch_gets_twenty_a_second_and_the_rest_as_one_dropped_at_the_tick() {
    let fake = Fake::default();
    let pipe = fake.pipe();
    let hub = Hub::new(fake);
    let dir = TempDir::new("watch-client-cap");
    let policy = policy(&dir);
    let a = launch();
    let _a = hub.register(sid(POOL_A), Arc::clone(&policy), Arc::clone(&a.sink));
    let mut burst = Vec::new();
    for n in 0..25 {
        burst.extend(denied(POOL_A, &format!(r"C:\f{n}"), "read"));
    }
    pipe.send(burst).unwrap();
    eventually("twenty passed", || a.keep.got().len() == 20);
    // The cap's window ends; the client's next tick delivers the count.
    a.clock.0.store(1500, Ordering::SeqCst);
    eventually("the count came", || a.keep.got().len() == 21);
    assert_eq!(a.keep.got().last(), Some(&Denial::Dropped { count: 5 }));
}

#[test]
fn an_unknown_access_is_a_write_where_reading_is_allowed_and_else_a_read() {
    let fake = Fake::default();
    {
        let mut script = fake.script();
        script
            .deny
            .insert(PathBuf::from(r"C:\readable"), Some(false));
        script.deny.insert(PathBuf::from(r"C:\refused"), Some(true));
        script.deny.insert(PathBuf::from(r"C:\untold"), None);
    }
    let pipe = fake.pipe();
    let hub = Hub::new(fake);
    let dir = TempDir::new("watch-client-unknown");
    let policy = policy(&dir);
    let a = launch();
    let _a = hub.register(sid(POOL_A), Arc::clone(&policy), Arc::clone(&a.sink));
    for path in [r"C:\readable", r"C:\refused", r"C:\untold"] {
        pipe.send(denied(POOL_A, path, "unknown")).unwrap();
    }
    eventually("three came", || a.keep.got().len() == 3);
    assert_eq!(
        a.keep.got(),
        [
            path_denial(&policy, r"C:\readable", Access::Write),
            path_denial(&policy, r"C:\refused", Access::Read),
            path_denial(&policy, r"C:\untold", Access::Read),
        ]
    );
}

#[test]
fn the_last_registration_dropping_closes_the_pipe() {
    let fake = Fake::default();
    let pipe = fake.pipe();
    let hub = Hub::new(fake);
    let dir = TempDir::new("watch-client-close");
    let (a, b) = (launch(), launch());
    let first = hub.register(sid(POOL_A), policy(&dir), Arc::clone(&a.sink));
    let second = hub.register(sid(POOL_B), policy(&dir), Arc::clone(&b.sink));
    drop(first);
    pipe.send(b"\n".to_vec()).unwrap();
    assert_eq!(hub.status(), Status::Live);
    drop(second);
    assert_eq!(hub.status(), Status::Off);
    eventually("the reader dropped the pipe", || {
        pipe.send(Vec::new()).is_err()
    });
}

#[test]
fn a_network_path_is_only_shown_and_an_unknown_one_a_read() {
    let fake = Fake::default();
    let pipe = fake.pipe();
    let hub = Hub::new(fake.clone());
    let dir = TempDir::new("watch-client-net");
    let policy = policy(&dir);
    let a = launch();
    let _a = hub.register(sid(POOL_A), Arc::clone(&policy), Arc::clone(&a.sink));
    pipe.send(line(POOL_A, r"\\server\share\a.docx", "unknown", true))
        .unwrap();
    pipe.send(line(POOL_A, r"\\server\share\b.docx", "write", true))
        .unwrap();
    // Not marked, but not a local drive path either: still never checked.
    pipe.send(denied(POOL_A, r"\\?\UNC\server\share\c", "unknown"))
        .unwrap();
    // Marked, on a letter: shown, not checked.
    pipe.send(line(POOL_A, r"Z:\mapped.txt", "unknown", true))
        .unwrap();
    eventually("four came", || a.keep.got().len() == 4);
    assert_eq!(
        a.keep.got(),
        [
            path_denial(&policy, r"\\server\share\a.docx", Access::Read),
            path_denial(&policy, r"\\server\share\b.docx", Access::Write),
            path_denial(&policy, r"\\?\UNC\server\share\c", Access::Read),
            path_denial(&policy, r"Z:\mapped.txt", Access::Read),
        ]
    );
    assert_eq!(fake.0.asked.lock().unwrap().len(), 0, "nothing was opened");
}

#[test]
fn a_path_is_shown_with_its_controls_and_bidi_characters_escaped() {
    let fake = Fake::default();
    let pipe = fake.pipe();
    let hub = Hub::new(fake);
    let dir = TempDir::new("watch-client-safe");
    let policy = policy(&dir);
    let a = launch();
    let _a = hub.register(sid(POOL_A), Arc::clone(&policy), Arc::clone(&a.sink));
    pipe.send(denied(POOL_A, "C:\\in\u{202E}fdp.exe\u{7}", "read"))
        .unwrap();
    eventually("it came", || a.keep.got().len() == 1);
    let Denial::Path { path, .. } = &a.keep.got()[0] else {
        panic!("a path");
    };
    assert_eq!(path, &PathBuf::from(r"C:\in\u{202E}fdp.exe\u{0007}"));
}

#[test]
fn only_a_local_drive_letter_path_is_one_the_check_may_open() {
    for good in [r"C:\", r"C:\Users\a b\x.txt", r"d:\x\y", r"C:\folder\"] {
        assert!(is_local_drive_path(Path::new(good)), "{good}");
    }
    for bad in [
        r"\\server\share\x",
        r"\\?\UNC\server\share",
        r"\\?\C:\x",
        r"\\.\pipe\x",
        r"\Device\Mup\x",
        r"//server/share",
        r"d:/x/y",
        r"C:\a/b",
        r"C:x.txt",
        r"C:",
        r"1:\x",
        r"C:\a\..\x",
        r"C:\a\.\x",
        r"C:\a\\server\x",
        "C:\\a\0b",
    ] {
        assert!(!is_local_drive_path(Path::new(bad)), "{bad}");
    }
}

#[test]
fn a_pause_is_told_to_every_launch_once_until_it_resumes() {
    let fake = Fake::default();
    let pipe = fake.pipe();
    let hub = Hub::new(fake);
    let dir = TempDir::new("watch-client-pause");
    let policy = policy(&dir);
    let (a, b) = (launch(), launch());
    let _a = hub.register(sid(POOL_A), Arc::clone(&policy), Arc::clone(&a.sink));
    let _b = hub.register(sid(POOL_B), Arc::clone(&policy), Arc::clone(&b.sink));
    let paused = b"{\"v\":1,\"paused\":{\"secs\":30,\"reason\":\"rate\"}}\n";
    pipe.send(paused.to_vec()).unwrap();
    // A second pause line while paused tells no one again.
    pipe.send(paused.to_vec()).unwrap();
    pipe.send(denied(POOL_A, r"C:\a", "read")).unwrap();
    eventually("a got the refusal", || a.keep.got().len() == 2);
    let notice = Denial::ReporterPaused { secs: 30 };
    assert_eq!(a.keep.got()[0], notice);
    assert_eq!(b.keep.got(), [notice]);
    // Resumed, then paused again: told again.
    pipe.send(b"{\"v\":1,\"resumed\":true}\n".to_vec()).unwrap();
    pipe.send(b"{\"v\":1,\"paused\":{\"secs\":5,\"reason\":\"rate\"}}\n".to_vec())
        .unwrap();
    eventually("b heard the second pause", || b.keep.got().len() == 2);
    assert_eq!(b.keep.got()[1], Denial::ReporterPaused { secs: 5 });
    assert_eq!(a.keep.got().len(), 3);
}

mod fallback;
mod local;
