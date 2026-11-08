//! The watcher's consumer, with no Windows call: what the ETW callback does with each
//! `Microsoft-Windows-Kernel-File` event (`windows_trace.rs` hands it the raw parts).
//!
//! - **Create (12)** carries `Irp, FileObject, IssuingThreadId, CreateOptions, CreateAttributes,
//!   ShareAccess, FileName`, and the opener's SID in its extended data
//!   (`EVENT_ENABLE_PROPERTY_SID`). One whose SID isn't a pool user's is dropped before its
//!   payload is read; at most its `Irp` evicts a pending pool Create the IRP was reused from.
//! - **`OperationEnd` (24)** carries `Irp, ExtraInformation, Status`. IRPs are reused, so a pool
//!   Create pairs with the next `OperationEnd` of its `Irp`; `Status` `STATUS_ACCESS_DENIED` makes
//!   it a [`Record`]. The pending map holds at most [`PENDING_CAP`] Creates, the oldest evicted.
//!
//! Only the versions the offsets were written for are read: Create v1 and `OperationEnd` v0, as
//! the spike recorded them on WS2025 26100 (`/tmp/asp-etw/logs/row1-sample-pair.xml`,
//! `results.md:53,67`). An event of another version is skipped and counted
//! ([`Counts::unknown_version`]), which the watcher logs once, as a count.
//!
//! Each Create keeps its `EventHeader.TimeStamp`; an `OperationEnd` stamped before the waiting
//! Create of its `Irp` belongs to an earlier use of the IRP and pairs with nothing. A pool
//! Create whose `FileName` is over [`MAX_NAME_UNITS`] is dropped, not truncated, and counted in
//! what the clients are told was dropped.
//!
//! The payloads are read at fixed offsets, not through TDH: the consumer sees every
//! `OperationEnd` on the machine (10⁴ a second during a build), and a TDH lookup per event would
//! cost more than the whole consumer does now (0.1 % of a core in the spike). Each read checks
//! the length, so a payload of another shape is dropped, never misread past its end.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use super::paths::DeviceMap;
use crate::windows::sids::Sid;

/// Kernel-File's provider GUID, `{edd08927-9cc4-4e65-b970-c2560fb5c289}`.
pub const KERNEL_FILE: u128 = 0xedd0_8927_9cc4_4e65_b970_c256_0fb5_c289;
/// Create.
pub const CREATE_ID: u16 = 12;
/// `OperationEnd`.
pub const END_ID: u16 = 24;
/// `STATUS_ACCESS_DENIED`.
pub const ACCESS_DENIED: u32 = 0xC000_0022;
/// The most pool Creates waiting for their `OperationEnd`.
pub const PENDING_CAP: usize = 4096;
/// The longest `FileName` kept, in UTF-16 units.
pub const MAX_NAME_UNITS: usize = 4096;
/// The event versions the offsets were written for: Create v1, `OperationEnd` v0.
pub const KNOWN_VERSIONS: [(u16, u8); 2] = [(CREATE_ID, 1), (END_ID, 0)];
/// `EVENT_HEADER_EXT_TYPE_SID`, the extended data item holding the user's SID.
pub const EXT_TYPE_SID: u16 = 2;

/// Whether event `id` of `version` is one the offsets were written for.
pub fn known_version(id: u16, version: u8) -> bool {
    KNOWN_VERSIONS.contains(&(id, version))
}

/// The SID item among an event's extended data items (`(ExtType, its bytes)`, `None` when the
/// item has no data): the first of type [`EXT_TYPE_SID`] with data. No other item is ever read
/// as a SID.
pub fn sid_in<'a>(items: impl IntoIterator<Item = (u16, Option<&'a [u8]>)>) -> Option<&'a [u8]> {
    items
        .into_iter()
        .find(|(kind, data)| *kind == EXT_TYPE_SID && data.is_some())
        .and_then(|(_, data)| data)
}

/// What the refused open asked for, as far as `CreateOptions` tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Not used today: a Create has no `DesiredAccess`, so a read is never certain.
    Read,
    /// Its disposition makes or replaces a file, or it deletes on close.
    Write,
    /// A plain open: the runner works it out (`AuthzAccessCheck`, part B).
    Unknown,
}

impl Access {
    /// Its name on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "write",
            Access::Unknown => "unknown",
        }
    }
}

/// `FILE_DELETE_ON_CLOSE`.
const DELETE_ON_CLOSE: u32 = 0x1000;
/// `FILE_OPEN`, the only disposition that neither makes nor replaces a file.
const FILE_OPEN: u32 = 1;

/// What `create_options` asks for: the disposition is its top byte (`FILE_SUPERSEDE` 0,
/// `FILE_OPEN` 1, `FILE_CREATE` 2, `FILE_OPEN_IF` 3, `FILE_OVERWRITE` 4, `FILE_OVERWRITE_IF`
/// 5). Anything but `FILE_OPEN`, or `FILE_DELETE_ON_CLOSE`, writes; a plain open is unknown.
pub fn access_of(create_options: u32) -> Access {
    let disposition = create_options >> 24;
    if disposition > 5 {
        Access::Unknown
    } else if disposition != FILE_OPEN || create_options & DELETE_ON_CLOSE != 0 {
        Access::Write
    } else {
        Access::Unknown
    }
}

/// A Create's fields the watcher keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Create {
    /// `Irp`.
    pub irp: u64,
    /// `CreateOptions`.
    pub options: u32,
    /// `FileName`: the NT path, `\Device\HarddiskVolumeN\...`.
    pub file_name: String,
}

/// An `OperationEnd`'s fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    /// `Irp`.
    pub irp: u64,
    /// `Status`, an NTSTATUS.
    pub status: u32,
}

/// A pointer field at `at` in `data`: 8 bytes when `wide`, else 4.
fn pointer(data: &[u8], at: usize, wide: bool) -> Option<u64> {
    if wide {
        let bytes = data.get(at..at + 8)?;
        Some(u64::from_le_bytes(bytes.try_into().ok()?))
    } else {
        u32_at(data, at).map(u64::from)
    }
}

/// A `u32` at `at` in `data`.
fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

/// The `Irp` of any Kernel-File payload: its first field.
pub fn irp_of(data: &[u8], wide: bool) -> Option<u64> {
    pointer(data, 0, wide)
}

/// A Create's payload, `wide` when its pointers are 8 bytes: `None` when it is too short, its
/// `FileName` has no terminating NUL, or the name's bytes are odd in number.
pub fn parse_create(data: &[u8], wide: bool) -> Option<Create> {
    let size = if wide { 8 } else { 4 };
    let irp = pointer(data, 0, wide)?;
    // Irp, FileObject, then IssuingThreadId and CreateOptions.
    let options = u32_at(data, 2 * size + 4)?;
    // Then CreateAttributes and ShareAccess, then FileName to its NUL.
    let name = data.get(2 * size + 16..)?;
    if name.len() % 2 != 0 {
        return None;
    }
    let (pairs, _odd) = name.as_chunks::<2>();
    let units: Vec<u16> = pairs
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    if units.len() * 2 + 2 > name.len() {
        return None;
    }
    Some(Create {
        irp,
        options,
        file_name: String::from_utf16_lossy(&units),
    })
}

/// An `OperationEnd`'s payload: `None` when it is too short.
pub fn parse_end(data: &[u8], wide: bool) -> Option<End> {
    let size = if wide { 8 } else { 4 };
    Some(End {
        irp: pointer(data, 0, wide)?,
        status: u32_at(data, 2 * size)?,
    })
}

/// `sid` in its binary form (`S-1-<authority>-<sub>...`): revision, count, a 48-bit authority
/// big-endian, then each sub-authority little-endian. `None` for text that isn't one.
pub fn sid_bytes(sid: &Sid) -> Option<Vec<u8>> {
    let mut parts = sid.as_str().split('-');
    if parts.next()? != "S" || parts.next()? != "1" {
        return None;
    }
    let authority: u64 = parts.next()?.parse().ok()?;
    if authority >> 48 != 0 {
        return None;
    }
    let subs = parts
        .map(str::parse::<u32>)
        .collect::<Result<Vec<u32>, _>>()
        .ok()?;
    let count = u8::try_from(subs.len()).ok().filter(|n| *n <= 15)?;
    let mut bytes = vec![1, count];
    bytes.extend_from_slice(&authority.to_be_bytes()[2..]);
    for sub in subs {
        bytes.extend_from_slice(&sub.to_le_bytes());
    }
    Some(bytes)
}

/// The pool users' SIDs, binary, for comparing each event's SID byte for byte.
#[derive(Debug, Clone)]
pub struct PoolSids(Vec<(Vec<u8>, Sid)>);

impl PoolSids {
    /// `pool`, each SID that has a binary form (a [`Sid`] always has).
    pub fn new(pool: &[Sid]) -> PoolSids {
        PoolSids(
            pool.iter()
                .filter_map(|sid| Some((sid_bytes(sid)?, sid.clone())))
                .collect(),
        )
    }

    /// The pool user whose binary SID is `bytes`.
    pub fn find(&self, bytes: &[u8]) -> Option<&Sid> {
        self.0
            .iter()
            .find(|(known, _)| known.as_slice() == bytes)
            .map(|(_, sid)| sid)
    }
}

/// One event as the callback reads it from its `EVENT_RECORD`.
#[derive(Debug, Clone, Copy)]
pub struct RawEvent<'a> {
    /// The event's id.
    pub id: u16,
    /// The header's `ProcessId`.
    pub pid: u32,
    /// The `EVENT_HEADER_EXT_TYPE_SID` item's bytes, when there is one.
    pub sid: Option<&'a [u8]>,
    /// The payload (`UserData`).
    pub data: &'a [u8],
    /// Whether its pointers are 8 bytes (`EVENT_HEADER_FLAG_64_BIT_HEADER`).
    pub wide: bool,
    /// `EventDescriptor.Version`.
    pub version: u8,
    /// `EventHeader.TimeStamp`.
    pub stamp: i64,
}

/// A pool user's refused open: what goes on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The pool user, as the record names it.
    pub sid: Sid,
    /// The opening process, from the event's header.
    pub pid: u32,
    /// The path with its drive letter (`C:\...`); for display only when [`Record::net`].
    pub path: String,
    /// Not under a known local volume (`paths.rs`): never to be opened or checked.
    pub net: bool,
    /// What the open asked for.
    pub access: Access,
}

/// A pool Create waiting for its `OperationEnd`.
#[derive(Debug, Clone)]
struct Pending {
    sid: Sid,
    pid: u32,
    file_name: String,
    access: Access,
    seq: u64,
    /// The Create's `EventHeader.TimeStamp`.
    stamp: i64,
}

/// The pool Creates waiting for their `OperationEnd`, by `Irp`, bounded.
#[derive(Debug)]
pub struct Pairing {
    pending: HashMap<u64, Pending>,
    /// Each insert, oldest first, with its sequence; an entry replaced since is stale.
    order: VecDeque<(u64, u64)>,
    next: u64,
    cap: usize,
}

impl Pairing {
    /// An empty map holding at most `cap`.
    pub fn new(cap: usize) -> Pairing {
        Pairing {
            pending: HashMap::new(),
            order: VecDeque::new(),
            next: 0,
            cap: cap.max(1),
        }
    }

    /// How many Creates wait.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Whether none waits.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// A pool Create, which replaces one of the same `Irp`; past the cap the oldest goes.
    fn create(&mut self, sid: Sid, pid: u32, stamp: i64, create: Create) {
        let seq = self.next;
        self.next += 1;
        let pending = Pending {
            sid,
            pid,
            stamp,
            access: access_of(create.options),
            file_name: create.file_name,
            seq,
        };
        self.pending.insert(create.irp, pending);
        self.order.push_back((create.irp, seq));
        while self.pending.len() > self.cap {
            let Some((irp, seq)) = self.order.pop_front() else {
                break;
            };
            if self.pending.get(&irp).is_some_and(|p| p.seq == seq) {
                self.pending.remove(&irp);
            }
        }
        if self.order.len() > 2 * self.cap {
            let pending = &self.pending;
            self.order
                .retain(|(irp, seq)| pending.get(irp).is_some_and(|p| p.seq == *seq));
        }
    }

    /// `irp` was reused by a Create that isn't a pool user's: its pending Create is gone.
    fn forget(&mut self, irp: u64) {
        self.pending.remove(&irp);
    }

    /// An `OperationEnd` stamped `stamp`: the Create it ends, when one waits and it was
    /// refused. One stamped before the waiting Create ends an earlier use of the IRP: the
    /// Create keeps waiting.
    fn end(&mut self, end: End, stamp: i64) -> Option<Pending> {
        if self.pending.get(&end.irp)?.stamp > stamp {
            return None;
        }
        let pending = self.pending.remove(&end.irp)?;
        (end.status == ACCESS_DENIED).then_some(pending)
    }
}

/// What the consumer counted, for the log line (counts only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Pool Creates kept.
    pub kept: u64,
    /// Refusals passed on.
    pub refused: u64,
    /// Refusals dropped: a path not under `\Device\`.
    pub unmapped: u64,
    /// Payloads too short to read.
    pub malformed: u64,
    /// Events of a version the offsets weren't written for, skipped.
    pub unknown_version: u64,
    /// Pool Creates whose `FileName` was over [`MAX_NAME_UNITS`], dropped.
    pub too_long: u64,
}

/// The callback's state: the pool, the pending Creates and the device map.
#[derive(Debug)]
pub struct Consumer {
    pool: PoolSids,
    pairing: Pairing,
    paths: DeviceMap,
    counts: Counts,
    /// Refusals lost here since [`Consumer::take_dropped`], for the clients' `dropped` count.
    dropped: u64,
}

impl Consumer {
    /// A consumer that keeps `pool`'s events.
    pub fn new(pool: &[Sid]) -> Consumer {
        Consumer {
            pool: PoolSids::new(pool),
            pairing: Pairing::new(PENDING_CAP),
            paths: DeviceMap::default(),
            counts: Counts::default(),
            dropped: 0,
        }
    }

    /// What was dropped here since the last call, which goes into the clients' `dropped`.
    pub fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.dropped)
    }

    /// What it counted so far.
    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// How many pool Creates wait.
    pub fn pending(&self) -> usize {
        self.pairing.len()
    }

    /// One Kernel-File event at `now`. A Create not of a pool user's is dropped here, its
    /// payload unread past its `Irp`; `out` is called only with a pool user's refusal.
    /// `devices` lists `(drive, device)` pairs when the device map misses.
    pub fn on_event(
        &mut self,
        event: &RawEvent<'_>,
        now: Duration,
        devices: &mut dyn FnMut() -> Vec<(char, String)>,
        out: &mut dyn FnMut(Record),
    ) {
        if !known_version(event.id, event.version) {
            if event.id == CREATE_ID || event.id == END_ID {
                self.counts.unknown_version += 1;
            }
            return;
        }
        match event.id {
            CREATE_ID => {
                let Some(sid) = event.sid.and_then(|bytes| self.pool.find(bytes)) else {
                    if !self.pairing.is_empty()
                        && let Some(irp) = irp_of(event.data, event.wide)
                    {
                        self.pairing.forget(irp);
                    }
                    return;
                };
                let sid = sid.clone();
                match parse_create(event.data, event.wide) {
                    Some(create) if create.file_name.encode_utf16().count() > MAX_NAME_UNITS => {
                        // Its IRP's earlier Create is gone, as this open isn't kept.
                        self.pairing.forget(create.irp);
                        self.counts.too_long += 1;
                        self.dropped += 1;
                    }
                    Some(create) => {
                        self.counts.kept += 1;
                        self.pairing.create(sid, event.pid, event.stamp, create);
                    }
                    None => self.counts.malformed += 1,
                }
            }
            END_ID if !self.pairing.is_empty() => {
                let Some(end) = parse_end(event.data, event.wide) else {
                    self.counts.malformed += 1;
                    return;
                };
                let Some(denied) = self.pairing.end(end, event.stamp) else {
                    return;
                };
                match self.paths.map_path(&denied.file_name, now, devices) {
                    Some(mapped) => {
                        self.counts.refused += 1;
                        out(Record {
                            sid: denied.sid,
                            pid: denied.pid,
                            path: mapped.path,
                            net: mapped.net,
                            access: denied.access,
                        });
                    }
                    None => self.counts.unmapped += 1,
                }
            }
            _ => {}
        }
    }
}
