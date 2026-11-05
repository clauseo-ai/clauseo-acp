//! The relay's limits: the input limits on each connection and the spend caps (starting values,
//! unmeasured). The turn caps are per session, held in [`TurnCaps`]; the day cap is per install,
//! held in [`DayCount`] and written by [`DayWriter`] to `<data_dir>/connectors/day.json`, so a
//! restart does not reset it.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::log_file::Cause;

/// The most paid `tools/call`s one turn (or one between-turns stretch) may send.
pub const MAX_PAID_CALLS_PER_TURN: u32 = 60;
/// The most paid `tools/call`s this install may send in one local day, across all sessions.
pub const MAX_PAID_CALLS_PER_DAY: u32 = 600;
/// The most `recharge` calls one turn (or one between-turns stretch) may send.
pub const MAX_RECHARGE_CALLS_PER_TURN: u32 = 5;
/// The longest line, either way, without its newline.
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
/// The longest connector reply line the relay passes on. S28 reads bodies up to 8 MB, so this
/// check is the relay's own.
pub const MAX_REPLY_BYTES: usize = 4 * 1024 * 1024;
/// The most raw line bytes one session may have in flight across its connections; one line alone
/// may take it whole. A parsed line can take about 36 times its size (a 4 MB array of zeros is
/// 144 MB of `Value`), so this, not the count in flight, bounds what an authenticated client can
/// make the host hold: about 150 MB per session, where 16 lines of 4 MB could reach 2.3 GB. The
/// agent's requests are a few kilobytes, so they never wait on it.
pub const MAX_IN_FLIGHT_BYTES_PER_SESSION: usize = 4 * 1024 * 1024;
/// The most messages one connection may have in flight; past it the relay stops reading.
pub const MAX_IN_FLIGHT: usize = 4;
/// The most connections one listener may hold waiting on their handshake.
pub const MAX_PENDING_HANDSHAKES: usize = 8;
/// The most connections past their handshake per token.
pub const MAX_CONNECTIONS_PER_TOKEN: usize = 4;
/// How long a handshake may take, from the accept to the hello's newline.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest hello line.
pub const MAX_HELLO_BYTES: usize = 1024;

/// What a `tools/call` spends, by its tool name. Any name but `account` and `recharge` is paid,
/// unknown names and other spellings included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spend {
    /// `account`: not counted.
    Free,
    /// `recharge`: its own cap.
    Recharge,
    /// Every other tool: the turn and the day.
    Paid,
}

impl Spend {
    pub(crate) fn of(tool: &str) -> Spend {
        match tool {
            "account" => Spend::Free,
            "recharge" => Spend::Recharge,
            _ => Spend::Paid,
        }
    }
}

/// Which cap a call ran into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cap {
    Turn,
    Day,
    Recharge,
}

/// One session's turn counts. A call while no turn is open counts against the between-turns
/// bucket, which has the same caps and resets only when the next turn starts.
#[derive(Debug, Default)]
pub(crate) struct TurnCaps {
    open: bool,
    paid: u32,
    recharge: u32,
    /// Bumped whenever a bucket starts, so a refund reaches only the bucket it was taken from.
    epoch: u64,
}

#[cfg(test)]
impl TurnCaps {
    /// Sets the paid count of the open turn, or of the between-turns bucket when none is open
    /// (tests), as that many paid calls would have: a cap is reached in a few real calls.
    pub(crate) fn set_paid_for_test(&mut self, paid: u32) {
        self.paid = paid;
    }
}

/// One counted call, as [`TurnCaps::refund`] needs it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Taken {
    spend: Spend,
    epoch: u64,
    /// The local day a paid call was counted on.
    day: String,
}

impl TurnCaps {
    /// A turn starts: the turn and the between-turns bucket both start again at 0.
    pub(crate) fn start_turn(&mut self) {
        *self = TurnCaps {
            open: true,
            paid: 0,
            recharge: 0,
            epoch: self.epoch.wrapping_add(1),
        };
    }

    /// The turn ends: what follows counts against a fresh between-turns bucket.
    pub(crate) fn end_turn(&mut self) {
        if self.open {
            *self = TurnCaps {
                epoch: self.epoch.wrapping_add(1),
                ..TurnCaps::default()
            };
        }
    }

    /// Takes one call as [`TurnCaps::take`] does, and says what to give back if it never goes out.
    pub(crate) fn take_call(
        &mut self,
        spend: Spend,
        day: &mut DayCount,
        today: &str,
    ) -> Result<Taken, Cap> {
        self.take(spend, day, today)?;
        Ok(Taken {
            spend,
            epoch: self.epoch,
            day: today.to_owned(),
        })
    }

    /// Gives back a call the connector surely did not charge: to its bucket, if that bucket is
    /// still the current one, and to its day, if that day is still the current one.
    pub(crate) fn refund(&mut self, taken: &Taken, day: &mut DayCount) {
        let same_bucket = taken.epoch == self.epoch;
        match taken.spend {
            Spend::Recharge if same_bucket => self.recharge = self.recharge.saturating_sub(1),
            Spend::Free | Spend::Recharge => {}
            Spend::Paid => {
                if same_bucket {
                    self.paid = self.paid.saturating_sub(1);
                }
                day.refund(&taken.day);
            }
        }
    }

    /// Takes one call of `spend` from the turn (or the between-turns bucket) and, for a paid
    /// call, from `day` on `today`. Nothing is counted when a cap refuses it.
    pub(crate) fn take(
        &mut self,
        spend: Spend,
        day: &mut DayCount,
        today: &str,
    ) -> Result<(), Cap> {
        match spend {
            Spend::Free => Ok(()),
            Spend::Recharge => {
                if self.recharge >= MAX_RECHARGE_CALLS_PER_TURN {
                    return Err(Cap::Recharge);
                }
                self.recharge = self.recharge.saturating_add(1);
                Ok(())
            }
            Spend::Paid => {
                if self.paid >= MAX_PAID_CALLS_PER_TURN {
                    return Err(Cap::Turn);
                }
                day.take(today)?;
                self.paid = self.paid.saturating_add(1);
                Ok(())
            }
        }
    }
}

/// `day.json`: the local day and its paid calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DayFile {
    day: String,
    paid: u32,
}

/// The install's paid calls today, in memory, under the day lock. [`DayWriter`] keeps
/// `day.json` up with it, outside that lock.
#[derive(Debug)]
pub(crate) struct DayCount {
    day: String,
    paid: u32,
    /// Bumped at every change, so the writer can tell a newer count from an older one. The
    /// count as loaded is 0, already on disk.
    seq: u64,
}

impl DayCount {
    /// Reads `path`. A missing file is a day with no calls; a file that does not read or parse
    /// is the same, with a warning (a same-user process can reset it anyway, which tier C
    /// accepts).
    pub(crate) fn load(path: &Path) -> DayCount {
        let read = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<DayFile>(&bytes)
                .ok()
                .filter(|file| {
                    let ok = is_day(&file.day);
                    if !ok {
                        tracing::warn!("connectors: day.json names no day; counting from 0");
                    }
                    ok
                }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                tracing::warn!(error = %Cause(&err), "connectors: day.json does not read; counting from 0");
                None
            }
        };
        let (day, paid) = read.map_or((String::new(), 0), |file| (file.day, file.paid));
        DayCount { day, paid, seq: 0 }
    }

    /// Takes one paid call on `today` (`YYYY-MM-DD`): a day at [`MAX_PAID_CALLS_PER_DAY`]
    /// refuses. A later day starts at 0. The day before the one held (a time zone crossed
    /// westward) counts against the day held, so travel cannot reset the cap; a day further back
    /// (a clock that once ran ahead and was set right, or a future `day.json`) starts at 0 too,
    /// so a wrong clock cannot hold the cap for years. In memory only: the caller has
    /// [`DayWriter::persist`] write it, after the lock and before the call goes out.
    pub(crate) fn take(&mut self, today: &str) -> Result<(), Cap> {
        if starts_over(&self.day, today) {
            today.clone_into(&mut self.day);
            self.paid = 0;
        }
        if self.paid >= MAX_PAID_CALLS_PER_DAY {
            return Err(Cap::Day);
        }
        self.paid = self.paid.saturating_add(1);
        self.seq = self.seq.wrapping_add(1);
        Ok(())
    }

    /// Gives back one paid call of `on`, if that is still the day held. In memory only, as
    /// [`DayCount::take`].
    pub(crate) fn refund(&mut self, on: &str) {
        if self.day == on && self.paid > 0 {
            self.paid -= 1;
            self.seq = self.seq.wrapping_add(1);
        }
    }

    /// The count's sequence number: what [`DayWriter::persist`] is asked to reach.
    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }
}

/// Writes `day.json` from the [`DayCount`], outside the day lock, so paid calls across
/// sessions do not queue on each other's fsyncs. Writes are one at a time, each of the newest
/// count, and never of a count older than the last written (so the file is never behind a call
/// that [`DayWriter::persist`] returned for). Calls that wait meanwhile are covered by the next
/// write, which holds their counts too: one write for many.
#[derive(Debug)]
pub(crate) struct DayWriter {
    path: PathBuf,
    /// The sequence number on disk, and the lock that makes writes one at a time.
    written: Mutex<u64>,
    /// The writes made (tests).
    #[cfg(test)]
    writes: std::sync::atomic::AtomicU64,
}

impl DayWriter {
    pub(crate) fn new(path: PathBuf) -> DayWriter {
        DayWriter {
            path,
            written: Mutex::new(0),
            #[cfg(test)]
            writes: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Returns once `day.json` holds count `seq` of `day` or a newer one. Takes `day`'s lock
    /// only to copy the count; the caller holds no lock. A failed write is logged and the count
    /// kept in memory; the next change tries again.
    pub(crate) fn persist(&self, day: &Mutex<DayCount>, seq: u64) {
        let mut written = self.written.lock().unwrap_or_else(PoisonError::into_inner);
        if *written >= seq {
            return;
        }
        let (newest, file) = {
            let day = day.lock().unwrap_or_else(PoisonError::into_inner);
            let file = DayFile {
                day: day.day.clone(),
                paid: day.paid,
            };
            (day.seq, file)
        };
        #[cfg(test)]
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let saved = serde_json::to_vec(&file)
            .map_err(std::io::Error::other)
            .and_then(|bytes| write_beside(&self.path, &bytes));
        match saved {
            Ok(()) => *written = newest,
            Err(err) => {
                tracing::warn!(error = %Cause(&err), "connectors: day.json could not be written");
            }
        }
    }
}

/// Whether `today` starts a new count after `held`: it is later, or more than one day earlier.
/// `YYYY-MM-DD` orders as text; the empty day of a fresh count is before any.
fn starts_over(held: &str, today: &str) -> bool {
    if today > held {
        return true;
    }
    match (days_from_civil(held), days_from_civil(today)) {
        (Some(held), Some(today)) => held.saturating_sub(today) > 1,
        _ => false,
    }
}

/// Days since 1970-01-01 of a `YYYY-MM-DD` date (Howard Hinnant's algorithm); `None` when it is
/// not one.
fn days_from_civil(text: &str) -> Option<i64> {
    if !is_day(text) {
        return None;
    }
    let year: i64 = text.get(0..4)?.parse().ok()?;
    let month: i64 = text.get(5..7)?.parse().ok()?;
    let day: i64 = text.get(8..10)?.parse().ok()?;
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Writes `bytes` to `path` atomically, making its folder first, then flushes the folder so
/// the rename itself survives a power cut. When this write made the folder, its parent is
/// flushed once too, so the folder's own entry survives.
fn write_beside(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let folder = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let made = !folder.is_dir();
    crate::write::create_dir_all(folder)?;
    if made
        && let Some(parent) = folder
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
    {
        sync_folder(parent)?;
    }
    crate::write::replace_private_file(path, bytes)?;
    sync_folder(folder)
}

#[cfg(test)]
thread_local! {
    /// The folder syncs this thread made (tests).
    pub(crate) static FOLDER_SYNCS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Flushes `folder`'s entries to disk, so a rename in it is durable. Unix: opens the folder and
/// fsyncs it (`sync_all`). Windows: nothing; std cannot open a folder for a flush (that needs
/// `FILE_FLAG_BACKUP_SEMANTICS`), and NTFS journals the rename's metadata.
pub(crate) fn sync_folder(folder: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    FOLDER_SYNCS.with(|syncs| syncs.set(syncs.get().saturating_add(1)));
    if cfg!(unix) {
        std::fs::File::open(folder)?.sync_all()
    } else {
        Ok(())
    }
}

/// The OS's time zone, as the day cap needs it (decision D5): desktop fills it from the OS, and
/// tests use a [`FixedOffset`] with the host's manual clock.
pub trait LocalDay: Send + Sync {
    /// Seconds east of UTC at `unix_secs` (India: 19,800).
    fn utc_offset_secs(&self, unix_secs: i64) -> i32;
}

/// A time zone with one fixed offset from UTC, in seconds east.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedOffset(pub i32);

impl LocalDay for FixedOffset {
    fn utc_offset_secs(&self, _unix_secs: i64) -> i32 {
        self.0
    }
}

/// The local date at `unix_millis` under `zone`, as `YYYY-MM-DD`.
pub(crate) fn local_date(unix_millis: i64, zone: &dyn LocalDay) -> String {
    let secs = unix_millis.div_euclid(1000);
    let local = secs.saturating_add(i64::from(zone.utc_offset_secs(secs)));
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

/// The proleptic Gregorian date of `days` since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Whether `text` is `YYYY-MM-DD` in digits.
fn is_day(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

#[cfg(test)]
mod tests;
