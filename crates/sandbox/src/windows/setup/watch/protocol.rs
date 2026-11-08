//! The watcher's pipe, as both ends see it, with no Windows call: its name, its descriptor, the
//! key that publishes the name, the lines it carries, and the rate they go out at.
//!
//! - **Name:** `\\.\pipe\clauseo-sandbox-watch-<32 hex>`, 128 random bits per run, published as
//!   [`PIPE_VALUE`] in `HKLM\`[`WATCH_KEY`] (64-bit view). The key is volatile (gone at a
//!   restart) and made fresh by each run with [`KEY_SDDL`]: SYSTEM writes it, Administrators
//!   and Users read it. The run deletes it as it exits, and `--remove` with the task. A key a
//!   killed run left is SYSTEM's, and the next run deletes it ([`key_start`]); one anyone else
//!   owns is refused (74). A name left by a crash names no pipe, or a squatter's: the client
//!   checks that the server's pipe is owned by SYSTEM before it reads a byte, as `SetUp`'s
//!   client does.
//! - **Descriptor** ([`pipe_sddl`]): owned by SYSTEM, protected, SYSTEM full control and the
//!   record's owner read only; nobody else, not a pool user. Duplex, as `PeekNamedPipe` needs read
//!   access on the server end; the client can't write all the same. Every instance after
//!   the first is made by this server while another is open (`instances.rs`), and its owner is
//!   checked; `FILE_FLAG_FIRST_PIPE_INSTANCE` refuses a name someone made first, and
//!   `PIPE_REJECT_REMOTE_CLIENTS` refuses the network.
//! - **Lines:** UTF-8 JSON, one per `\n`: a refusal, `{"v":1,"sid":"S-1-5-21-…","pid":1234,
//!   "path":"C:\\…","access":"read"|"write"|"unknown"}`, or what was dropped since the last
//!   such line, `{"v":1,"dropped":n}`. A refusal outside every known local volume (a share, a
//!   web folder, a device with no letter) adds `"net":true` and its `path` is for display
//!   only: the client must never open or check it, which would reach a server a pool user chose
//!   with the lawyer's credentials. `net` is always sent, and read as true when absent (fail
//!   safe: a path of unknown kind is never opened), so `v` stays 1.
//!   `dropped` counts the refusals the rate held back, those the consumer's channel had no room
//!   for, Creates whose name was too long, and the events ETW itself lost. When the event-rate
//!   breaker trips (`breaker.rs`), `{"v":1,"paused":{"secs":60,"reason":"rate"}}`: no refusal
//!   is seen for `secs`; then `{"v":1,"resumed":true}`. The client reads; it never writes.
//! - **Rate:** at most [`RECORDS_PER_SECOND`] refusals a second; the rest are counted and sent
//!   as one `dropped` line when the second ends (the runner caps at 20 a second).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::consumer::{Access, Record};
use crate::windows::pipe::{PIPE_ROOT, hex};
use crate::windows::sids::Sid;

/// The protocol's version, in every line.
pub const VERSION: u32 = 1;
/// The start of the pipe's name, after `\\.\pipe\`.
pub const PIPE_PREFIX: &str = "clauseo-sandbox-watch-";
/// The key that publishes the pipe's name, under `HKLM` (64-bit view).
pub const WATCH_KEY: &str = r"SOFTWARE\Clauseo\SandboxWatch";
/// The `REG_SZ` value holding the pipe's full name.
pub const PIPE_VALUE: &str = "PipeName";
/// The key's descriptor: owned by SYSTEM, protected; SYSTEM full control, Administrators and
/// Users read. Nobody but SYSTEM can point a client at another pipe.
pub const KEY_SDDL: &str = "O:SYD:P(A;;KA;;;SY)(A;;KR;;;BA)(A;;KR;;;BU)";
/// The most refusals sent in one [`RATE_WINDOW`].
pub const RECORDS_PER_SECOND: u32 = 200;
/// The rate's window.
pub const RATE_WINDOW: Duration = Duration::from_secs(1);

/// What a run does with the [`WATCH_KEY`] it finds at its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStart {
    /// There is none: make it.
    Fresh,
    /// SYSTEM owns it: a killed run's (one task instance runs at a time), deleted and made
    /// again.
    Stale,
    /// Someone else owns it: refused (74).
    Foreign,
}

/// What to do with the key, given whether its owner is SYSTEM (`None`: there is no key).
pub fn key_start(owner_is_system: Option<bool>) -> KeyStart {
    match owner_is_system {
        None => KeyStart::Fresh,
        Some(true) => KeyStart::Stale,
        Some(false) => KeyStart::Foreign,
    }
}

/// The pipe's full name for the random `bits`.
pub fn pipe_name(bits: &[u8; 16]) -> String {
    format!("{PIPE_ROOT}{PIPE_PREFIX}{}", hex(bits))
}

/// Whether `name`, read from [`PIPE_VALUE`], is a name [`pipe_name`] makes: the client opens
/// nothing else.
pub fn is_pipe_name(name: &str) -> bool {
    name.strip_prefix(PIPE_ROOT)
        .and_then(|rest| rest.strip_prefix(PIPE_PREFIX))
        .is_some_and(|hex| {
            hex.len() == 32
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

/// The pipe's descriptor for the record's owner `owner`: SYSTEM owns it and has full control,
/// `owner` reads (`FILE_GENERIC_READ`: no write, no new instance), and a Medium label with
/// no-read-up keeps the owner's Low processes out.
pub fn pipe_sddl(owner: &Sid) -> String {
    format!("O:SYD:P(A;;FA;;;SY)(A;;FR;;;{owner})S:(ML;;NWNR;;;ME)")
}

/// One line, as the client parses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Line {
    /// A refusal.
    Denied {
        /// [`VERSION`].
        v: u32,
        /// The pool user.
        sid: String,
        /// The opening process.
        pid: u32,
        /// The path, with its drive letter or as UNC.
        path: String,
        /// `read`, `write` or `unknown`.
        access: String,
        /// Not under a known local volume: `path` is for display only.
        #[serde(default = "net_when_absent")]
        net: bool,
    },
    /// Refusals dropped since the last such line.
    Dropped {
        /// [`VERSION`].
        v: u32,
        /// How many.
        dropped: u64,
    },
    /// The breaker tripped: nothing is seen for a while.
    Paused {
        /// [`VERSION`].
        v: u32,
        /// How long, and why.
        paused: Paused,
    },
    /// The breaker's pause ended.
    Resumed {
        /// [`VERSION`].
        v: u32,
        /// Always true.
        resumed: bool,
    },
}

/// `net` when a line leaves it out: true, so a client never opens a path of unknown kind.
fn net_when_absent() -> bool {
    true
}

/// A pause: how long, and why (`rate` today).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paused {
    /// Seconds.
    pub secs: u64,
    /// Why.
    pub reason: String,
}

impl Line {
    /// The line `text` (without its `\n`): `None` for anything else, or another version.
    pub fn parse(text: &str) -> Option<Line> {
        let line: Line = serde_json::from_str(text).ok()?;
        let v = match &line {
            Line::Denied { v, .. }
            | Line::Dropped { v, .. }
            | Line::Paused { v, .. }
            | Line::Resumed { v, .. } => *v,
        };
        (v == VERSION).then_some(line)
    }
}

/// `record` as a line with its `\n`, only when its SID is one of `pool`'s: the writer's own
/// check that no one else's refusal goes out, whatever the consumer did.
pub fn denied_line(record: &Record, pool: &[Sid]) -> Option<String> {
    if !pool.contains(&record.sid) {
        return None;
    }
    let line = Line::Denied {
        v: VERSION,
        sid: record.sid.as_str().to_owned(),
        pid: record.pid,
        path: record.path.clone(),
        access: record.access.as_str().to_owned(),
        net: record.net,
    };
    serde_json::to_string(&line).ok().map(|text| text + "\n")
}

/// A `dropped` line with its `\n`.
pub fn dropped_line(dropped: u64) -> String {
    format!("{{\"v\":{VERSION},\"dropped\":{dropped}}}\n")
}

/// A `paused` line for a pause of `secs` for the event rate, with its `\n`.
pub fn paused_line(secs: u64) -> String {
    format!("{{\"v\":{VERSION},\"paused\":{{\"secs\":{secs},\"reason\":\"rate\"}}}}\n")
}

/// A `resumed` line with its `\n`.
pub fn resumed_line() -> String {
    format!("{{\"v\":{VERSION},\"resumed\":true}}\n")
}

/// `access` back from its name on the wire.
pub fn access_named(name: &str) -> Option<Access> {
    [Access::Read, Access::Write, Access::Unknown]
        .into_iter()
        .find(|access| access.as_str() == name)
}

/// The source's rate limit: [`RECORDS_PER_SECOND`] per [`RATE_WINDOW`], the rest counted.
#[derive(Debug, Clone)]
pub struct RateLimit {
    began: Duration,
    sent: u32,
    dropped: u64,
}

impl RateLimit {
    /// A limit whose first window begins at `now`.
    pub fn new(now: Duration) -> RateLimit {
        RateLimit {
            began: now,
            sent: 0,
            dropped: 0,
        }
    }

    /// Whether one more refusal may go out at `now`; one that may not is counted.
    pub fn admit(&mut self, now: Duration) -> bool {
        if now.saturating_sub(self.began) >= RATE_WINDOW {
            self.began = now;
            self.sent = 0;
        }
        if self.sent < RECORDS_PER_SECOND {
            self.sent += 1;
            true
        } else {
            self.dropped += 1;
            false
        }
    }

    /// Counts `n` refusals lost before the limit saw them (the consumer's channel was full).
    pub fn lose(&mut self, n: u64) {
        self.dropped = self.dropped.saturating_add(n);
    }

    /// Once the window that began has ended at `now`: what was dropped, when anything was,
    /// which is then cleared.
    pub fn roll(&mut self, now: Duration) -> Option<u64> {
        if now.saturating_sub(self.began) < RATE_WINDOW || self.dropped == 0 {
            return None;
        }
        self.began = now;
        self.sent = 0;
        Some(std::mem::take(&mut self.dropped))
    }
}
