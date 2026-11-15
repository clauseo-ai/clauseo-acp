//! The browser's `DevTools` channel, on every system: the guard holds both ends of the browser's
//! `DevTools` pipe (`chrome/pipe.rs`) and relays it to agent-browser over a websocket on
//! 127.0.0.1 (route 2, decision record 0060).
//!
//! The browser has no debugging port: nothing but the guard can reach it. The relay listens on
//! 127.0.0.1 at a port the OS picks (`devtools/listen.rs`; on Windows no other socket may share
//! it), and lets in a connection only when both locks open:
//!
//! 1. the process at its other end is agent-browser or one of its descendants (its daemon),
//!    found by the socket's owner (`devtools/peer.rs`), checked before a byte is read; until the
//!    guard names agent-browser ([`Devtools::allow_descendants_of`]) nothing is let in;
//! 2. it asks for exactly the websocket upgrade on `/devtools/browser/<secret>`, the secret 256
//!    bits from the OS's random source in hex (`devtools/handshake.rs`). Every other request,
//!    `/json` included, gets a 404 and a close.
//!
//! One client at a time: agent-browser's daemon, given [`Devtools::url`] as `AGENT_BROWSER_CDP`
//! in its environment, never on a command line. While it is connected, any other upgrade is
//! refused the same way.
//!
//! Websocket text frames from the client become NUL-ended messages on the pipe, and NUL-ended
//! messages from the pipe become text frames (`devtools/frames.rs`, `devtools/nul.rs`), through
//! [`routes::Routes`], which also carries the guard's own CDP traffic: the relay's commands
//! ([`Devtools::command`]) and the guard's clients (`devtools/seam.rs`, the contract stage D's
//! proxy-auth answer plugs into), attached and given their start commands before agent-browser
//! starts ([`Devtools::start_guard`]).
//!
//! One `Devtools` lives as long as the browser it relays: a stop ends both, and the next start
//! makes a new pipe, a new port and a new secret.

mod frames;
mod handshake;
mod listen;
mod nul;
mod peer;
mod routes;
pub(crate) mod seam;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Chain, Cursor, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::{Map, Value};

pub(crate) use routes::Reply;
use routes::{FromClient, Routes, ToGuard};
use seam::{CdpLink, GuardClient};

/// How long a connection has to send its request head.
const HANDSHAKE_WAIT: Duration = Duration::from_secs(5);
/// How long a write to the client may block before the client is dropped.
const CLIENT_WRITE_WAIT: Duration = Duration::from_secs(30);
/// How many connections may be in their handshake at once; more are closed unread.
const MAX_HANDSHAKES: usize = 8;

/// The relay for one browser. Dropping it closes the client and the listener.
pub(crate) struct Devtools {
    core: Arc<Core>,
    port: u16,
    secret: Arc<str>,
}

/// What the relay's threads share.
struct Core {
    to_browser: Mutex<Box<dyn Write + Send>>,
    routes: Mutex<Routes>,
    client: Mutex<Option<Client>>,
    clients: AtomicU64,
    handshakes: AtomicUsize,
    stopped: AtomicBool,
    browser_gone: AtomicBool,
    /// agent-browser's pid, whose descendants may connect; none until the guard names it.
    root: Mutex<Option<u32>>,
    /// The guard's clients (`seam.rs`), attached before agent-browser starts.
    guard_clients: Mutex<Vec<Arc<dyn GuardClient>>>,
}

/// The relay's side of the seam: the guard's messages, written to the browser's pipe.
struct PipeLink(
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read only by PR-3's ProxyAuth, through send")
    )]
    Arc<Core>,
);

impl CdpLink for PipeLink {
    fn send(&self, message: &Value) -> io::Result<()> {
        nul::write(
            &mut *lock(&self.0.to_browser),
            message.to_string().as_bytes(),
        )
    }
}

/// The connected client.
struct Client {
    id: u64,
    stream: TcpStream,
}

impl Devtools {
    /// Starts relaying the browser's pipe: `to_browser` is the end the browser reads,
    /// `from_browser` the end it writes.
    pub(crate) fn start(
        to_browser: Box<dyn Write + Send>,
        from_browser: Box<dyn Read + Send>,
    ) -> io::Result<Devtools> {
        let secret: Arc<str> = Arc::from(handshake::new_secret()?);
        let listener = listen::loopback()?;
        let port = listener.local_addr()?.port();
        let core = Arc::new(Core {
            to_browser: Mutex::new(to_browser),
            routes: Mutex::new(Routes::default()),
            client: Mutex::new(None),
            clients: AtomicU64::new(0),
            handshakes: AtomicUsize::new(0),
            stopped: AtomicBool::new(false),
            browser_gone: AtomicBool::new(false),
            root: Mutex::new(None),
            guard_clients: Mutex::new(Vec::new()),
        });
        {
            let core = Arc::clone(&core);
            // Never joined: it ends when the browser closes its end, which its kill does.
            drop(thread::spawn(move || pump_browser(&core, from_browser)));
        }
        {
            let (core, secret) = (Arc::clone(&core), Arc::clone(&secret));
            // Never joined: the drop wakes it and it ends.
            drop(thread::spawn(move || {
                accept(&core, &listener, port, &secret);
            }));
        }
        Ok(Devtools { core, port, secret })
    }

    /// `ws://127.0.0.1:<port>/devtools/browser/<secret>`, for agent-browser's environment.
    pub(crate) fn url(&self) -> String {
        format!(
            "ws://127.0.0.1:{}/devtools/browser/{}",
            self.port, self.secret
        )
    }

    /// Lets the process `pid` (agent-browser) and its descendants connect, from now on.
    pub(crate) fn allow_descendants_of(&self, pid: u32) {
        *lock(&self.core.root) = Some(pid);
    }

    /// The relay's port, for the guard's notes (the secret is never logged).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the tests only, for now")
    )]
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// Sends a guard command (an id from [`GUARD_IDS`]) on `session` (none: the browser's
    /// own) and returns where its reply arrives. agent-browser never sees the reply.
    pub(crate) fn command(
        &self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> io::Result<(i64, Receiver<Reply>)> {
        let (reply, inbox) = mpsc::channel();
        let id = seam::next_guard_id();
        lock(&self.core.routes).await_reply(id, reply);
        let mut message = Map::new();
        message.insert("id".to_owned(), Value::from(id));
        message.insert("method".to_owned(), Value::from(method));
        message.insert("params".to_owned(), params);
        if let Some(session) = session {
            message.insert("sessionId".to_owned(), Value::from(session));
        }
        let text = Value::Object(message).to_string();
        if let Err(error) = nul::write(&mut *lock(&self.core.to_browser), text.as_bytes()) {
            lock(&self.core.routes).forget(id);
            return Err(error);
        }
        Ok((id, inbox))
    }

    /// [`Devtools::command`], waiting at most `wait` for the reply.
    pub(crate) fn call(
        &self,
        method: &str,
        params: Value,
        session: Option<&str>,
        wait: Duration,
    ) -> Result<Value, String> {
        let (id, inbox) = self
            .command(method, params, session)
            .map_err(|error| format!("cannot write to the browser's pipe: {error}"))?;
        match inbox.recv_timeout(wait) {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(error)) => Err(format!("the browser refused {method}: {error}")),
            Err(RecvTimeoutError::Timeout) => {
                lock(&self.core.routes).forget(id);
                Err(format!("the browser did not answer {method}"))
            }
            Err(RecvTimeoutError::Disconnected) => Err("the browser closed its pipe".to_owned()),
        }
    }

    /// Attaches the guard's clients for this browser (`seam::clients`, given agent-browser's
    /// environment) and sends their start commands, each answered within
    /// [`seam::START_COMMAND_WAIT`]. Called before agent-browser starts; an error fails the
    /// browser's start.
    pub(crate) fn start_guard(&self, env: &BTreeMap<String, OsString>) -> Result<(), String> {
        self.attach(&seam::clients(&self.link(), env))
    }

    /// The guard's link to this browser (`seam::CdpLink`).
    pub(crate) fn link(&self) -> Arc<dyn CdpLink> {
        Arc::new(PipeLink(Arc::clone(&self.core)))
    }

    /// [`Devtools::start_guard`]'s work for the given clients.
    pub(crate) fn attach(&self, clients: &[Arc<dyn GuardClient>]) -> Result<(), String> {
        lock(&self.core.routes).take_events(
            clients
                .iter()
                .flat_map(|client| client.events().iter().copied()),
        );
        *lock(&self.core.guard_clients) = clients.to_vec();
        for client in clients {
            for (method, params) in client.start_commands() {
                self.call(&method, params, None, seam::START_COMMAND_WAIT)?;
            }
        }
        Ok(())
    }
}

impl Drop for Devtools {
    fn drop(&mut self) {
        self.core.stopped.store(true, Ordering::SeqCst);
        self.core.drop_client(None);
        // Wakes the accept loop, which then sees `stopped`.
        if let Ok(wake) = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)) {
            drop(wake);
        }
    }
}

impl Core {
    /// Writes a text frame to the client, if it is still client `only` (any client: `None`).
    /// A failed write drops the client.
    fn to_client(&self, only: Option<u64>, text: &[u8]) {
        let mut client = lock(&self.client);
        let Some(current) = client.as_mut() else {
            return;
        };
        if only.is_some_and(|id| id != current.id) {
            return;
        }
        if frames::write_text(&mut current.stream, text).is_err() {
            close(&current.stream);
            *client = None;
        }
    }

    /// Answers client `id`'s ping.
    fn pong(&self, id: u64, payload: &[u8]) {
        let mut client = lock(&self.client);
        if let Some(current) = client.as_mut().filter(|current| current.id == id)
            && frames::write_pong(&mut current.stream, payload).is_err()
        {
            close(&current.stream);
            *client = None;
        }
    }

    /// Closes and forgets the client, if it is still client `only` (any client: `None`).
    fn drop_client(&self, only: Option<u64>) {
        let mut client = lock(&self.client);
        if let Some(current) = client.as_mut()
            && only.is_none_or(|id| id == current.id)
        {
            // Best effort: the client may be gone already.
            drop(frames::write_close(&mut current.stream));
            close(&current.stream);
            *client = None;
        }
    }
}

/// Reads the browser's messages until its pipe ends, routing each.
fn pump_browser(core: &Core, from_browser: Box<dyn Read + Send>) {
    let mut reader = nul::Reader::new(from_browser);
    while let Ok(Some(message)) = reader.next() {
        let routed = lock(&core.routes).browser_message(&message);
        if routed.forward {
            core.to_client(None, &message);
        }
        if routed.guard != ToGuard::Nothing {
            // Outside every lock: a client may send through its link from here.
            let clients = lock(&core.guard_clients).clone();
            for client in &clients {
                match &routed.guard {
                    ToGuard::Reply(id, reply) => client.on_reply(*id, reply),
                    ToGuard::Event(event) => client.on_event(event),
                    ToGuard::Nothing => {}
                }
            }
        }
    }
    core.browser_gone.store(true, Ordering::SeqCst);
    // Pending guard calls see their sender drop.
    *lock(&core.routes) = Routes::default();
    core.drop_client(None);
}

/// Accepts connections until the relay stops, each on a thread of its own.
fn accept(core: &Arc<Core>, listener: &TcpListener, port: u16, secret: &Arc<str>) {
    for stream in listener.incoming() {
        if core.stopped.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else {
            continue;
        };
        if core.handshakes.fetch_add(1, Ordering::SeqCst) >= MAX_HANDSHAKES {
            core.handshakes.fetch_sub(1, Ordering::SeqCst);
            close(&stream);
            continue;
        }
        let (core, secret) = (Arc::clone(core), Arc::clone(secret));
        drop(thread::spawn(move || serve(&core, stream, port, &secret)));
    }
}

/// One connection: its handshake, then, for the one accepted client, its messages.
fn serve(core: &Core, mut stream: TcpStream, port: u16, secret: &str) {
    let root = *lock(&core.root);
    if !root.is_some_and(|root| peer::allowed(&stream, root)) {
        core.handshakes.fetch_sub(1, Ordering::SeqCst);
        close(&stream);
        return;
    }
    let handshake = handshake_of(core, &mut stream, port, secret);
    core.handshakes.fetch_sub(1, Ordering::SeqCst);
    let Some((id, leftover)) = handshake else {
        close(&stream);
        return;
    };
    let mut reader = frames::Reader::new(Cursor::new(leftover).chain(stream));
    relay_client(core, id, &mut reader);
    core.drop_client(Some(id));
}

/// Reads the request head and answers it: the 101 and the client's id and the bytes read past
/// the head, for the one accepted upgrade while no client is connected; otherwise a 404.
fn handshake_of(
    core: &Core,
    stream: &mut TcpStream,
    port: u16,
    secret: &str,
) -> Option<(u64, Vec<u8>)> {
    stream.set_read_timeout(Some(HANDSHAKE_WAIT)).ok()?;
    let mut head = Vec::new();
    let mut chunk = [0_u8; 1024];
    let end = loop {
        if let Some(end) = handshake::head_end(&head) {
            break end;
        }
        if head.len() > handshake::MAX_HEAD {
            not_found(stream);
            return None;
        }
        match stream.read(&mut chunk) {
            // A bare probe (agent-browser's first connection) or a stalled client.
            Ok(0) | Err(_) => return None,
            Ok(read) => head.extend_from_slice(chunk.get(..read).unwrap_or_default()),
        }
    };
    let leftover = head.split_off(end);
    let handshake::Verdict::Upgrade(reply) = handshake::judge(&head, port, secret) else {
        not_found(stream);
        return None;
    };
    let mut client = lock(&core.client);
    let refused = client.is_some()
        || core.stopped.load(Ordering::SeqCst)
        || core.browser_gone.load(Ordering::SeqCst);
    if refused {
        drop(client);
        not_found(stream);
        return None;
    }
    let ready = stream.set_read_timeout(None).is_ok()
        && stream.set_write_timeout(Some(CLIENT_WRITE_WAIT)).is_ok()
        && stream.write_all(&reply).is_ok();
    let writer = stream.try_clone().ok().filter(|_| ready)?;
    let id = core.clients.fetch_add(1, Ordering::SeqCst);
    *client = Some(Client { id, stream: writer });
    Some((id, leftover))
}

/// Relays client `id`'s messages to the browser until it closes or errs.
fn relay_client(
    core: &Core,
    id: u64,
    reader: &mut frames::Reader<Chain<Cursor<Vec<u8>>, TcpStream>>,
) {
    loop {
        match reader.next() {
            Ok(frames::Message::Text(text)) => {
                let routed = lock(&core.routes).client_message(&text);
                match routed {
                    FromClient::Forward(text) => {
                        if nul::write(&mut *lock(&core.to_browser), text.as_bytes()).is_err() {
                            return;
                        }
                    }
                    FromClient::Refuse(reply) => core.to_client(Some(id), reply.as_bytes()),
                    FromClient::Drop => {}
                }
            }
            Ok(frames::Message::Ping(payload)) => core.pong(id, &payload),
            Ok(frames::Message::Pong) => {}
            Ok(frames::Message::Close) | Err(_) => return,
        }
    }
}

/// The 404, then the close (the caller drops the stream).
fn not_found(stream: &mut TcpStream) {
    // Best effort: the peer may be gone.
    drop(stream.write_all(handshake::NOT_FOUND));
    close(stream);
}

/// Shuts both directions of `stream`; one already shut is fine.
fn close(stream: &TcpStream) {
    drop(stream.shutdown(Shutdown::Both));
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod peer_tests;
#[cfg(test)]
mod seam_tests;
#[cfg(test)]
mod tests;
