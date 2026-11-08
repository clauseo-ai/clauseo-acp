//! The app's ask of the set-up task, its order and its waits with no Windows call: `Asker`'s
//! seams are `windows::setup_task`'s calls, and the tests' fakes. Tested on every OS.
//!
//! 1. reads the published name ([`published`]) before each start of the task, so a name already
//!    there is known as one this start didn't make: a server already running, or a stale name a
//!    run that was ended left;
//! 2. starts the task;
//! 3. reads the published name until there is one, opening the pipe it names: a busy pipe is
//!    waited for; a missing one, or none published, is read again after a pause, the task
//!    started once more after [`RERUN_AFTER`], up to [`CONNECT_WAIT`];
//! 4. on that pipe the asker checks the server (its owner LOCAL SYSTEM first), then sends the
//!    request and reads the reply. A pipe that fails the check, or refuses this user, by a name
//!    known before the start is taken as stale (anyone may make a name once it was published
//!    and its server is gone) and the name read again; by a name this start published, it is a
//!    squat: [`SetUpError::Squatted`], or [`SetUpError::NotTheServer`] when a pool user owns it;
//! 5. a server that went before replying is asked again, once, after another start.

use super::{CONNECT_WAIT, RERUN_AFTER, Reply, SetUpError, published::pipe_name};

/// What opening a pipe by its name gave.
#[derive(Debug)]
pub enum Opened<C> {
    /// It opened: the connection.
    Open(C),
    /// No pipe has the name (`ERROR_FILE_NOT_FOUND`).
    Missing,
    /// Every instance is serving someone (`ERROR_PIPE_BUSY`).
    Busy,
    /// It refuses this user (`ERROR_ACCESS_DENIED`): not the set-up task's pipe.
    Denied,
    /// Anything else: the Win32 error.
    Failed(u32),
}

/// How an exchange on an open pipe failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exchange {
    /// The server closed before any reply.
    Gone,
    /// The server isn't the set-up task's: a pool user's (`serve::pool_squat`).
    NotTheServer(String),
    /// Anything else.
    Failed(String),
    /// The server isn't the set-up task's, nor a pool user's.
    Squatted(String),
}

/// The calls [`ask_with`] makes.
pub trait Asker {
    /// An open pipe.
    type Pipe;
    /// Starts the set-up task.
    fn start(&mut self) -> Result<(), SetUpError>;
    /// The published value as it is now: `None` when there is none.
    fn published(&mut self) -> Result<Option<String>, String>;
    /// A monotonic clock, in milliseconds.
    fn now_ms(&mut self) -> u64;
    /// The pause between two reads while there is no pipe to open.
    fn pause(&mut self);
    /// Opens the pipe `name`: identification only, the client rights alone, overlapped.
    fn open(&mut self, name: &str) -> Opened<Self::Pipe>;
    /// Waits a bounded time for an instance of the busy pipe `name`.
    fn wait_busy(&mut self, name: &str);
    /// Checks the server on `pipe` (its owner first), then sends the request and reads the reply.
    fn exchange(&mut self, pipe: Self::Pipe) -> Result<Reply, Exchange>;
}

/// The published value as a pipe name: `None` for none, and for a value that isn't a token,
/// which only an administrator could have written; it is logged and read as none.
fn published(value: Option<String>) -> Option<(String, String)> {
    let value = value?;
    let name = pipe_name(&value);
    if name.is_none() {
        tracing::warn!("the published set-up pipe name isn't a well-formed token");
    }
    name.map(|name| (value, name))
}

/// What a wait that ran out ends as: why the last look found no server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Last {
    /// No name was published, or only one known before the start.
    Unpublished,
    /// A name was published but its pipe was missing or busy.
    NoServer,
}

/// The module's order over `asker`.
pub fn ask_with<A: Asker>(asker: &mut A) -> Result<Reply, SetUpError> {
    let mut before = start(asker)?;
    let mut began = asker.now_ms();
    let mut started_again = false;
    let mut asked_again = false;
    let mut last = Last::Unpublished;
    loop {
        let elapsed = asker.now_ms().saturating_sub(began);
        if elapsed >= CONNECT_WAIT {
            return Err(match last {
                Last::Unpublished => SetUpError::Unpublished,
                Last::NoServer => SetUpError::NoServer,
            });
        }
        let found = asker
            .published()
            .map_err(|why| SetUpError::Pipe(format!("the published name can't be read: {why}")))?;
        let Some((token, name)) = published(found) else {
            last = Last::Unpublished;
            again(asker, elapsed, &mut started_again, &mut before)?;
            continue;
        };
        // A name this start published can't have been made first by anyone: it was unknown.
        let fresh = before.as_deref() != Some(token.as_str());
        let pipe = match asker.open(&name) {
            Opened::Open(pipe) => pipe,
            Opened::Busy => {
                last = Last::NoServer;
                asker.wait_busy(&name);
                continue;
            }
            Opened::Missing => {
                last = Last::NoServer;
                again(asker, elapsed, &mut started_again, &mut before)?;
                continue;
            }
            Opened::Denied if fresh => {
                // The set-up task's pipe lets this user in: one that doesn't is someone else's,
                // whose owner can't be read, so nothing is stopped for it.
                return Err(SetUpError::Squatted(
                    "the pipe refuses this user, so another process made it".to_owned(),
                ));
            }
            Opened::Denied => {
                last = Last::Unpublished;
                again(asker, elapsed, &mut started_again, &mut before)?;
                continue;
            }
            Opened::Failed(code) => {
                return Err(SetUpError::Pipe(format!(
                    "opening it failed (error {code})"
                )));
            }
        };
        match asker.exchange(pipe) {
            Ok(reply) => return Ok(reply),
            Err(Exchange::Gone) if !asked_again => {
                asked_again = true;
                before = start(asker)?;
                began = asker.now_ms();
                started_again = false;
            }
            Err(Exchange::Gone) => {
                return Err(SetUpError::Pipe("the server went away twice".to_owned()));
            }
            Err(Exchange::NotTheServer(why) | Exchange::Squatted(why)) if !fresh => {
                tracing::info!(%why, "the set-up pipe name read before the start is stale");
                last = Last::Unpublished;
                again(asker, elapsed, &mut started_again, &mut before)?;
            }
            Err(Exchange::NotTheServer(why)) => return Err(SetUpError::NotTheServer(why)),
            Err(Exchange::Squatted(why)) => return Err(SetUpError::Squatted(why)),
            Err(Exchange::Failed(why)) => return Err(SetUpError::Pipe(why)),
        }
    }
}

/// Reads the published token, then starts the task: the token known before the start.
fn start<A: Asker>(asker: &mut A) -> Result<Option<String>, SetUpError> {
    let before = match asker.published() {
        Ok(found) => published(found).map(|(token, _name)| token),
        Err(why) => {
            tracing::warn!(%why, "the published set-up pipe name can't be read");
            None
        }
    };
    asker.start()?;
    Ok(before)
}

/// A pause before the next look, the task started once more after [`RERUN_AFTER`] (a start
/// made as the last server ended may have been dropped), once.
fn again<A: Asker>(
    asker: &mut A,
    elapsed: u64,
    started_again: &mut bool,
    before: &mut Option<String>,
) -> Result<(), SetUpError> {
    if elapsed >= RERUN_AFTER && !*started_again {
        *started_again = true;
        *before = start(asker)?;
    }
    asker.pause();
    Ok(())
}
