//! The app's ask through a fake machine (`client.rs`): the name the server publishes, read
//! before and after the start; a stale name, gone or someone else's, passed over for the new
//! one; a name this start published that a non-SYSTEM owner holds, refused as a squat.

use std::collections::{HashMap, VecDeque};

use super::*;

/// One run's token, by a byte.
fn token_of(byte: u8) -> String {
    token(&[byte; TOKEN_BYTES])
}

/// What a pipe does when the app opens it.
#[derive(Clone)]
enum Server {
    /// It opens: its owner, and what the exchange gives once the owner passes.
    Serves(Result<Sid, String>, Result<Reply, Exchange>),
    /// It refuses this user.
    Denies,
}

/// LOCAL SYSTEM's pipe, answering `reply`.
fn system(reply: Result<Reply, Exchange>) -> Server {
    Server::Serves(Ok(sid("S-1-5-18")), reply)
}

/// The machine as the app sees it: the published value, what each start publishes, the
/// pipes there are, the clock, and every call.
struct FakeAsker {
    value: Option<String>,
    /// What each start leaves published, in order; `None`, or none left, leaves it as it is
    /// (a start dropped while a server runs, or one that never served).
    starts: VecDeque<Option<String>>,
    pipes: HashMap<String, Server>,
    now: u64,
    calls: Vec<String>,
}

impl FakeAsker {
    fn new(value: Option<String>, starts: Vec<Option<String>>) -> FakeAsker {
        FakeAsker {
            value,
            starts: starts.into(),
            pipes: HashMap::new(),
            now: 0,
            calls: Vec::new(),
        }
    }

    fn with(mut self, token: &str, server: Server) -> FakeAsker {
        self.pipes.insert(pipe_name(token).unwrap(), server);
        self
    }

    fn count(&self, call: &str) -> usize {
        self.calls
            .iter()
            .filter(|made| made.as_str() == call)
            .count()
    }
}

impl Asker for FakeAsker {
    type Pipe = Server;

    fn start(&mut self) -> Result<(), SetUpError> {
        self.calls.push("start".to_owned());
        if let Some(Some(token)) = self.starts.pop_front() {
            self.value = Some(token);
        }
        Ok(())
    }

    fn published(&mut self) -> Result<Option<String>, String> {
        Ok(self.value.clone())
    }

    fn now_ms(&mut self) -> u64 {
        self.now
    }

    fn pause(&mut self) {
        self.now += 250;
    }

    fn open(&mut self, name: &str) -> Opened<Server> {
        self.calls.push(format!("open {name}"));
        match self.pipes.get(name) {
            Some(Server::Denies) => Opened::Denied,
            Some(server) => Opened::Open(server.clone()),
            None => Opened::Missing,
        }
    }

    fn wait_busy(&mut self, _name: &str) {
        self.now += 1_000;
    }

    fn exchange(&mut self, pipe: Server) -> Result<Reply, Exchange> {
        let Server::Serves(owner, reply) = pipe else {
            return Err(Exchange::Failed(
                "a pipe that refuses never opens".to_owned(),
            ));
        };
        // The owner first, as `setup_task::check_pipe` reads it: nothing is sent to another's.
        if let Err(why) = check_owner(&owner) {
            return Err(if pool_squat(&owner, &[sid(POOL)], &[sid(GROUP)]) {
                Exchange::NotTheServer(why)
            } else {
                Exchange::Squatted(why)
            });
        }
        self.calls.push("sent".to_owned());
        reply
    }
}

#[test]
fn the_app_reads_the_name_the_start_published_and_asks_on_its_pipe() {
    let fresh = token_of(1);
    let mut asker =
        FakeAsker::new(None, vec![Some(fresh.clone())]).with(&fresh, system(Ok(done())));
    assert_eq!(ask_with(&mut asker), Ok(done()));
    assert_eq!(asker.count("start"), 1);
    assert_eq!(asker.count("sent"), 1);
    // A server already running (the start dropped): its name, read before, is asked on.
    let running = token_of(2);
    let mut asker =
        FakeAsker::new(Some(running.clone()), Vec::new()).with(&running, system(Ok(done())));
    assert_eq!(ask_with(&mut asker), Ok(done()));
    // A value that isn't a token names nothing and isn't opened.
    let mut asker = FakeAsker::new(Some("..\\..\\x".to_owned()), vec![Some(fresh.clone())])
        .with(&fresh, system(Ok(done())));
    assert_eq!(ask_with(&mut asker), Ok(done()));
    assert!(
        asker.calls.iter().all(|call| !call.contains("..")),
        "{:?}",
        asker.calls
    );
}

#[test]
fn a_stale_name_gone_or_someone_elses_is_passed_over_for_the_one_the_start_publishes() {
    let (stale, fresh) = (token_of(1), token_of(2));
    // A run that was ended left its name; someone made a pipe under it since. The new run's
    // name, published a moment later (the first start dropped), is the one asked on.
    for squatter in [
        Server::Serves(Ok(sid(OTHER)), Ok(done())),
        Server::Serves(Ok(sid(POOL)), Ok(done())),
        Server::Denies,
    ] {
        let mut asker = FakeAsker::new(Some(stale.clone()), vec![None, Some(fresh.clone())])
            .with(&stale, squatter)
            .with(&fresh, system(Ok(done())));
        assert_eq!(ask_with(&mut asker), Ok(done()));
        assert_eq!(asker.count("start"), 2, "started again after RERUN_AFTER");
        assert_eq!(asker.count("sent"), 1, "sent only to the new server");
    }
    // A stale name whose pipe is gone, then the new one.
    let mut asker = FakeAsker::new(Some(stale.clone()), vec![Some(fresh.clone())])
        .with(&fresh, system(Ok(done())));
    assert_eq!(ask_with(&mut asker), Ok(done()));
    // Only the stale name, someone else's, and no new run: no name in time, nothing sent.
    let mut asker = FakeAsker::new(Some(stale.clone()), Vec::new())
        .with(&stale, Server::Serves(Ok(sid(OTHER)), Ok(done())));
    assert_eq!(ask_with(&mut asker), Err(SetUpError::Unpublished));
    assert_eq!(asker.count("sent"), 0);
    assert!(asker.now >= CONNECT_WAIT);
    // Only the stale name, its pipe gone: no server in time.
    let mut asker = FakeAsker::new(Some(stale), Vec::new());
    assert_eq!(ask_with(&mut asker), Err(SetUpError::NoServer));
    // Nothing ever published.
    let mut asker = FakeAsker::new(None, Vec::new());
    assert_eq!(ask_with(&mut asker), Err(SetUpError::Unpublished));
    assert_eq!(asker.count("start"), 2);
}

#[test]
fn a_name_the_start_published_held_by_an_owner_other_than_system_is_a_squat_sent_nothing() {
    let fresh = token_of(3);
    let squat = |server: Server| {
        let mut asker =
            FakeAsker::new(Some(token_of(1)), vec![Some(fresh.clone())]).with(&fresh, server);
        let answer = ask_with(&mut asker);
        assert_eq!(asker.count("sent"), 0, "nothing sent: {answer:?}");
        assert_eq!(asker.count("start"), 1, "refused at once: {answer:?}");
        answer
    };
    let other = squat(Server::Serves(Ok(sid(OTHER)), Ok(done())));
    let Err(SetUpError::Squatted(why)) = &other else {
        panic!("{other:?}");
    };
    assert!(why.contains("isn't owned by LOCAL SYSTEM"), "{why}");
    let unread = squat(Server::Serves(Err("denied".to_owned()), Ok(done())));
    assert!(matches!(unread, Err(SetUpError::Squatted(_))), "{unread:?}");
    let denies = squat(Server::Denies);
    assert!(matches!(denies, Err(SetUpError::Squatted(_))), "{denies:?}");
    // A pool user's: for `retry_squatted`, which stops the pool and asks once more.
    let pool = squat(Server::Serves(Ok(sid(POOL)), Ok(done())));
    assert!(matches!(pool, Err(SetUpError::NotTheServer(_))), "{pool:?}");
}

#[test]
fn a_server_that_went_before_replying_is_asked_again_once_by_the_next_runs_name() {
    let (first, second) = (token_of(1), token_of(2));
    let mut asker = FakeAsker::new(None, vec![Some(first.clone()), Some(second.clone())])
        .with(&first, system(Err(Exchange::Gone)))
        .with(&second, system(Ok(done())));
    assert_eq!(ask_with(&mut asker), Ok(done()));
    assert_eq!(asker.count("start"), 2);
    let mut asker = FakeAsker::new(None, vec![Some(first.clone()), Some(second.clone())])
        .with(&first, system(Err(Exchange::Gone)))
        .with(&second, system(Err(Exchange::Gone)));
    assert!(matches!(ask_with(&mut asker), Err(SetUpError::Pipe(_))));
}
