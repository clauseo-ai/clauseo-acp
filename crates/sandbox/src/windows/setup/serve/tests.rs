//! The set-up task's server through fakes, on every OS: who may run it, the pipe it makes, the
//! order each client is served in, and what each reply says. The gate's rules and the app's
//! check of the server are in `tests/identify.rs`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::windows::setup::record_values::CopyOwner;

mod app;
mod flow;
mod identify;

pub(crate) const LAWYER: &str = "S-1-5-21-1-2-3-1001";
pub(crate) const OTHER: &str = "S-1-5-21-1-2-3-1002";
pub(crate) const POOL: &str = "S-1-5-21-1-2-3-1010";
pub(crate) const GROUP: &str = "S-1-5-21-1-2-3-1050";
/// A set-up request.
pub(crate) const SET_UP: &str = r#"{"v":1,"op":"set_up"}"#;
const ADMIN_EXE: &str = r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe";

pub(crate) fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// `user`'s token in session 1 at Medium, unrestricted, in no group of the pool's.
pub(crate) fn client(user: &str) -> ClientFacts {
    ClientFacts {
        user: sid(user),
        session: 1,
        groups: vec![sid("S-1-5-32-545"), sid("S-1-5-4"), sid("S-1-1-0")],
        restricted: false,
        integrity: 0x2000,
    }
}

/// A per-machine record with a pool of one and its group.
pub(crate) fn machine_gate() -> Gate {
    Gate {
        copy_owner: Some(CopyOwner::Machine),
        pool: vec![sid(POOL)],
        groups: vec![sid(GROUP)],
    }
}

/// A connection: what it sends, and who it is.
pub(crate) struct FakeClient {
    pub(crate) request: Result<Vec<u8>, String>,
    pub(crate) facts: Result<ClientFacts, String>,
}

impl FakeClient {
    /// `user`'s request `request`.
    pub(crate) fn asking(user: &str, request: &str) -> FakeClient {
        FakeClient {
            request: Ok(request.as_bytes().to_vec()),
            facts: Ok(client(user)),
        }
    }
}

/// The pipe: clients in the order they connect, and the replies sent.
#[derive(Default)]
pub(crate) struct FakePipe {
    pub(crate) waiting: VecDeque<FakeClient>,
    pub(crate) waits: Vec<Duration>,
    pub(crate) replies: Vec<Reply>,
    pub(crate) fail_next: bool,
    /// The next wait finds the name someone else's.
    pub(crate) taken_next: bool,
}

impl Pipe for FakePipe {
    type Client = FakeClient;

    fn next(&mut self, wait: Duration) -> Result<Option<FakeClient>, Unmade> {
        self.waits.push(wait);
        if self.fail_next {
            return Err(Unmade::Failed("ConnectNamedPipe failed".to_owned()));
        }
        if self.taken_next {
            return Err(Unmade::Taken);
        }
        Ok(self.waiting.pop_front())
    }

    fn request(&mut self, client: &mut FakeClient) -> Result<Vec<u8>, String> {
        client.request.clone()
    }

    fn facts(&mut self, client: &mut FakeClient) -> Result<ClientFacts, String> {
        client.facts.clone()
    }

    fn reply(&mut self, _client: FakeClient, line: &[u8]) {
        assert_eq!(line.last(), Some(&b'\n'), "one line");
        self.replies.push(Reply::parse(line).unwrap());
    }
}

/// The host: what each read gives, and every call, in order.
pub(crate) struct FakeHost {
    pub(crate) system: bool,
    pub(crate) exe: PathBuf,
    pub(crate) gate: Result<Gate, String>,
    /// The gate after the lock is taken, when it changed meanwhile.
    pub(crate) gate_after_lock: Option<Gate>,
    /// Each session's user.
    pub(crate) sessions: Vec<(u32, Sid)>,
    /// Each user's profile.
    pub(crate) profiles: Vec<(Sid, PathBuf)>,
    /// Whether the lock comes within its wait.
    pub(crate) lock_free: bool,
    /// The users whose set-up is current.
    pub(crate) current: Vec<Sid>,
    /// What a set-up gives.
    pub(crate) result: SetupResult,
    /// The data folder's resolution fails with this.
    pub(crate) data_dir_fails: Option<(SetupExit, String)>,
    pub(crate) held: bool,
    pub(crate) calls: Vec<String>,
    pub(crate) lines: Vec<(bool, String)>,
}

impl FakeHost {
    /// SYSTEM from the admin copy, a per-machine record, the lawyer signed in to session 1.
    pub(crate) fn new() -> FakeHost {
        FakeHost {
            system: true,
            exe: PathBuf::from(ADMIN_EXE),
            gate: Ok(machine_gate()),
            gate_after_lock: None,
            sessions: vec![(1, sid(LAWYER)), (2, sid(OTHER))],
            profiles: vec![
                (sid(LAWYER), PathBuf::from(r"C:\Users\lawyer")),
                (sid(OTHER), PathBuf::from(r"C:\Users\other")),
            ],
            lock_free: true,
            current: Vec::new(),
            result: SetupResult::done(),
            data_dir_fails: None,
            held: false,
            calls: Vec::new(),
            lines: Vec::new(),
        }
    }

    /// The calls that changed or read a user's files, or ran a set-up.
    pub(crate) fn acted(&self) -> Vec<&str> {
        let acting = ["profile", "owner", "data_dir", "lock", "current", "set_up"];
        self.calls
            .iter()
            .map(String::as_str)
            .filter(|call| acting.iter().any(|name| call.starts_with(name)))
            .collect()
    }
}

/// The owner a fake set-up runs for: who and in which session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FakeOwner(Sid, u32);

impl Host for FakeHost {
    type Owner = FakeOwner;

    fn is_system(&mut self) -> Result<bool, String> {
        Ok(self.system)
    }

    fn current_exe(&mut self) -> Result<PathBuf, String> {
        Ok(self.exe.clone())
    }

    fn admin_copy(&mut self) -> Result<PathBuf, String> {
        Ok(PathBuf::from(ADMIN_EXE))
    }

    fn gate(&mut self) -> Result<Gate, String> {
        self.calls.push("gate".to_owned());
        match (&self.gate_after_lock, self.held) {
            (Some(after), true) => Ok(after.clone()),
            _ => self.gate.clone(),
        }
    }

    fn session_user(&mut self, session: u32) -> Result<Sid, String> {
        self.calls.push(format!("session_user {session}"));
        let found = self.sessions.iter().find(|(at, _)| *at == session);
        found
            .map(|(_, user)| user.clone())
            .ok_or_else(|| "WTSQueryUserToken failed".to_owned())
    }

    fn profile(&mut self, user: &Sid) -> Result<Option<PathBuf>, String> {
        self.calls.push(format!("profile {user}"));
        let found = self.profiles.iter().find(|(at, _)| at == user);
        Ok(found.map(|(_, profile)| profile.clone()))
    }

    fn owner(&mut self, session: u32, user: &Sid) -> Result<FakeOwner, String> {
        self.calls.push(format!("owner {session} {user}"));
        Ok(FakeOwner(user.clone(), session))
    }

    fn data_dir(&mut self, owner: &FakeOwner, path: &Path) -> Result<PathBuf, (SetupExit, String)> {
        // As the owner: the real one resolves it with the session's token.
        self.calls
            .push(format!("data_dir as {} {}", owner.0, path.display()));
        match &self.data_dir_fails {
            Some(failure) => Err(failure.clone()),
            None => Ok(path.to_path_buf()),
        }
    }

    fn lock(&mut self) -> Result<bool, String> {
        assert!(!self.held, "taken once per client");
        self.calls.push("lock".to_owned());
        self.held = self.lock_free;
        Ok(self.lock_free)
    }

    fn unlock(&mut self) {
        self.calls.push("unlock".to_owned());
        self.held = false;
    }

    fn current(&mut self, owner: &FakeOwner, _data_dir: &Path) -> bool {
        assert!(self.held, "read under the lock");
        self.calls.push(format!("current {}", owner.0));
        self.current.contains(&owner.0)
    }

    fn set_up(&mut self, owner: FakeOwner, data_dir: &Path, add: u8) -> SetupResult {
        assert!(
            self.held,
            "the set-up runs under the lock the server waited for"
        );
        self.calls
            .push(format!("set_up {} {} {add}", owner.0, data_dir.display()));
        self.result.clone()
    }

    fn log(&mut self, error: bool, line: &str) {
        self.lines.push((error, line.to_owned()));
    }
}

/// Serves `clients` and gives the replies, in order, with the exit.
fn serve_all(
    host: &mut FakeHost,
    clients: Vec<FakeClient>,
) -> (ServeExit, Vec<Reply>, Vec<Duration>) {
    let mut pipe = FakePipe {
        waiting: clients.into(),
        ..FakePipe::default()
    };
    let (exit, _sddl) = run_on(host, &mut pipe);
    (exit, pipe.replies, pipe.waits)
}

/// [`run`] on `pipe`, kept for its record: the exit, and the descriptor the pipe was made
/// with (none when it wasn't).
fn run_on(host: &mut FakeHost, pipe: &mut FakePipe) -> (ServeExit, Option<String>) {
    let taken = std::mem::take(pipe);
    let mut out = None;
    let mut sddl = None;
    let exit = run(host, |made| {
        sddl = Some(made.to_owned());
        Ok(Recording(taken, &mut out))
    });
    *pipe = out.unwrap_or_default();
    (exit, sddl)
}

/// A [`FakePipe`] that hands itself back when dropped.
struct Recording<'a>(FakePipe, &'a mut Option<FakePipe>);

impl Drop for Recording<'_> {
    fn drop(&mut self) {
        *self.1 = Some(std::mem::take(&mut self.0));
    }
}

impl Pipe for Recording<'_> {
    type Client = FakeClient;

    fn next(&mut self, wait: Duration) -> Result<Option<FakeClient>, Unmade> {
        self.0.next(wait)
    }

    fn request(&mut self, client: &mut FakeClient) -> Result<Vec<u8>, String> {
        self.0.request(client)
    }

    fn facts(&mut self, client: &mut FakeClient) -> Result<ClientFacts, String> {
        self.0.facts(client)
    }

    fn reply(&mut self, client: FakeClient, line: &[u8]) {
        self.0.reply(client, line);
    }
}

fn done() -> Reply {
    Reply::Ran(SetupResult::done())
}

#[test]
fn a_set_up_request_runs_the_set_up_for_the_session_user_in_their_data_folder() {
    let mut host = FakeHost::new();
    let (exit, replies, waits) = serve_all(&mut host, vec![FakeClient::asking(LAWYER, SET_UP)]);
    assert_eq!(exit, ServeExit::Done);
    assert_eq!(replies, [done()]);
    assert_eq!(
        waits,
        [FIRST_WAIT, IDLE_WAIT],
        "first client, then idle until none"
    );
    let data = Path::new(r"C:\Users\lawyer")
        .join("AppData")
        .join("Local")
        .join("ai.clauseo.desktop");
    let data = data.display();
    assert_eq!(
        host.acted(),
        [
            format!("profile {LAWYER}").as_str(),
            &format!("owner 1 {LAWYER}"),
            &format!("data_dir as {LAWYER} {data}"),
            "lock",
            &format!("current {LAWYER}"),
            &format!("set_up {LAWYER} {data} 0"),
        ]
    );
    assert!(!host.held, "the lock is given back");
    assert_eq!(
        host.lines,
        [(false, "set-up request (set_up): done".to_owned())]
    );
}

#[test]
fn the_app_reads_a_reply_as_it_reads_the_exes_result() {
    use crate::{Blocked, SetupError};
    assert_eq!(done().outcome(), Ok(()));
    let other = Reply::Ran(SetupResult::failed(SetupExit::OtherOwner, "owner", "D5"));
    let Err(SetupError::Failed(words)) = other.outcome() else {
        panic!("{other:?}");
    };
    assert!(words.contains("another Windows user"), "{words}");
    let blocked = Reply::Ran(SetupResult {
        blocked: Some(Blocked::LogonRight),
        ..SetupResult::failed(SetupExit::Failed, "logging a sandbox user on", "1385")
    });
    assert_eq!(
        blocked.outcome(),
        Err(SetupError::Blocked(Blocked::LogonRight))
    );
    let refused = Reply::Refused(Refusal::NotSessionUser.to_string());
    let Err(SetupError::Failed(words)) = refused.outcome() else {
        panic!("{refused:?}");
    };
    assert!(words.contains("isn't its session's user"), "{words}");
}

#[test]
fn a_client_that_closes_before_the_server_reaches_it_is_let_go_and_the_next_served() {
    use std::cell::Cell;
    // Each look at the clock is a second on.
    let ticks = Cell::new(0u64);
    let now = || {
        ticks.set(ticks.get() + 1);
        Duration::from_secs(ticks.get())
    };
    // ERROR_NO_DATA twice, then a client that stays: two re-arms, one connection.
    let mut answers = VecDeque::from([Attempt::Gone, Attempt::Gone, Attempt::Connected]);
    let (mut asked, mut rearmed) = (Vec::new(), 0);
    let wait = Duration::from_secs(30);
    let connected = connect_within(
        wait,
        now,
        |left| {
            asked.push(left);
            Ok::<_, ()>(answers.pop_front().unwrap())
        },
        || {
            rearmed += 1;
            Ok(())
        },
    );
    assert_eq!(connected, Ok(true));
    assert_eq!(rearmed, 2);
    // Each wait has only what is left of the whole.
    assert_eq!(asked, [29, 27, 25].map(Duration::from_secs));
    // A client coming and going for ever ends with the server's time, not before; none came.
    ticks.set(0);
    let mut tries = 0;
    let gone = connect_within(
        Duration::from_secs(10),
        now,
        |_| {
            tries += 1;
            Ok::<_, ()>(Attempt::Gone)
        },
        || Ok(()),
    );
    assert_eq!((gone, tries), (Ok(false), 5));
    assert_eq!(
        connect_within(wait, now, |_| Ok::<_, ()>(Attempt::TimedOut), || Ok(())),
        Ok(false)
    );
    // A re-arm that fails is the wait's failure.
    assert_eq!(
        connect_within(
            wait,
            now,
            |_| Ok(Attempt::Gone),
            || Err("DisconnectNamedPipe")
        ),
        Err("DisconnectNamedPipe")
    );
}
