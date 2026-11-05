//! The files under `<data_dir>/counts/`, and how each is read and written. Reads use std; every
//! change goes through [`crate::write`]. A file that cannot be read as its type is removed, never
//! guessed at.
//!
//! - `install_id`: the install id, as its 36 characters.
//! - `today.json`: the install id it was counted under, the open day's cells, its foreground
//!   seconds, the onboarding steps this install has counted and the last day a day report was
//!   taken.
//! - `queue/<YYYY-MM-DD>.json`: a closed day's report, waiting to be sent.
//! - `tally/<YYYY-Www>.json`: a week's tally, open (still counting) or closed (waiting to be sent),
//!   with the install id it was counted under (kept on disk only; the tally on the wire has none).
//!
//! `today.json`, a queued report or a tally made under another install id (left by an off whose
//! delete failed part way) is removed when the folder is read, never adopted by the current id.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::schema::{
    Day, DayCells, DayReport, InstallId, OnboardingStep, TallyCells, TallyReport, Time, Week,
};
use crate::write;

/// The folder under the data folder that holds every counts file.
pub(crate) const FOLDER: &str = "counts";
/// The first-act marker's name, in the data folder itself (coordinator decision 8). Never
/// deleted. Desktop checks it exists before `Counts::open`.
pub const FIRST_ACT: &str = "first_act_done";

/// `today.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TodayFile {
    /// The install id it was counted under.
    pub(crate) install: InstallId,
    /// The open day, once a tick has said which it is.
    pub(crate) day: Option<Day>,
    /// Its cells so far (without `time_open`, which the close makes).
    pub(crate) cells: DayCells,
    /// Its foreground seconds so far.
    pub(crate) foreground_secs: u64,
    /// The onboarding steps this install has counted, each once.
    pub(crate) steps: BTreeSet<OnboardingStep>,
    /// The last day a day report was taken (a 2xx or a `400`), so one goes a day.
    pub(crate) posted: Option<Day>,
}

/// `tally/<week>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum TallyFile {
    /// The week is still counting.
    Open {
        /// The install id it was counted under.
        install: InstallId,
        /// The week.
        week: Week,
        /// Its cells so far.
        cells: TallyCells,
    },
    /// The week has closed and its report waits for `send_at`.
    Closed {
        /// The install id it was counted under.
        install: InstallId,
        /// The report.
        report: TallyReport,
        /// When to send it.
        send_at: Time,
    },
}

impl TallyFile {
    fn week(&self) -> Week {
        match self {
            TallyFile::Open { week, .. } => *week,
            TallyFile::Closed { report, .. } => report.week,
        }
    }

    fn install(&self) -> InstallId {
        match self {
            TallyFile::Open { install, .. } | TallyFile::Closed { install, .. } => *install,
        }
    }
}

/// One file change, which the state queues under its lock and `Counts` makes with it released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    /// Write the first-act marker.
    Marker,
    /// Write `install_id`.
    Install(InstallId),
    /// Write `today.json`.
    Today(TodayFile),
    /// Write a tally file.
    Tally(TallyFile),
    /// Write a queued day.
    Queued(DayReport),
    /// Remove a queued day.
    RemoveQueued(Day),
    /// Remove a tally file.
    RemoveTally(Week),
    /// Delete the counts folder ([`Store::delete_all`]).
    DeleteAll,
}

/// The counts folder of one data folder.
#[derive(Debug, Clone)]
pub(crate) struct Store {
    root: PathBuf,
    marker: PathBuf,
}

impl Store {
    pub(crate) fn new(data_dir: &Path) -> Store {
        Store {
            root: data_dir.join(FOLDER),
            marker: data_dir.join(FIRST_ACT),
        }
    }

    /// Makes the change `op` names.
    pub(crate) fn apply(&self, op: &Op) -> io::Result<()> {
        match op {
            Op::Marker => self.write_marker(),
            Op::Install(id) => self.write_install(*id),
            Op::Today(today) => write_json(&self.today_path(), today),
            Op::Tally(tally) => write_json(&self.tally_path(tally.week()), tally),
            Op::Queued(report) => write_json(&self.queue_path(report.day), report),
            Op::RemoveQueued(day) => write::remove_file(&self.queue_path(*day)),
            Op::RemoveTally(week) => write::remove_file(&self.tally_path(*week)),
            Op::DeleteAll => self.delete_all(),
        }
    }

    /// Writes the first-act marker.
    fn write_marker(&self) -> io::Result<()> {
        write::replace(&self.marker, b"")
    }

    /// Deletes the counts folder and everything in it, `install_id` first: an off that fails
    /// part way never leaves the id behind with no way to tell, and the next id made drops
    /// every file left under the old one. While `install_id` cannot be removed, nothing else is.
    pub(crate) fn delete_all(&self) -> io::Result<()> {
        remove_if_folder(&self.install_path())?;
        write::remove_file(&self.install_path())?;
        write::remove_dir_all(&self.root)
    }

    fn install_path(&self) -> PathBuf {
        self.root.join("install_id")
    }

    fn today_path(&self) -> PathBuf {
        self.root.join("today.json")
    }

    fn queue_path(&self, day: Day) -> PathBuf {
        self.root.join("queue").join(format!("{day}.json"))
    }

    fn tally_path(&self, week: Week) -> PathBuf {
        self.root.join("tally").join(format!("{week}.json"))
    }

    /// The saved install id, if there is a readable one. A folder in its place is removed, so
    /// the new id can be written.
    pub(crate) fn read_install(&self) -> Option<InstallId> {
        remove_if_folder(&self.install_path()).ok()?;
        let bytes = std::fs::read(self.install_path()).ok()?;
        std::str::from_utf8(&bytes).ok()?.trim().parse().ok()
    }

    pub(crate) fn write_install(&self, id: InstallId) -> io::Result<()> {
        write::replace(&self.install_path(), id.to_string().as_bytes())
    }

    /// The saved open day counted under `install`; a missing or unreadable file, or one from
    /// another id (removed), reads as none.
    pub(crate) fn read_today(&self, install: InstallId) -> io::Result<Option<TodayFile>> {
        match read_json::<TodayFile>(&self.today_path())? {
            Some(today) if today.install == install => Ok(Some(today)),
            Some(_) => write::remove_file(&self.today_path()).map(|()| None),
            None => Ok(None),
        }
    }

    /// Every queued day whose file holds that day's report, made under `install`.
    pub(crate) fn read_queue(&self, install: InstallId) -> io::Result<BTreeMap<Day, DayReport>> {
        let mut queue = BTreeMap::new();
        for path in self.list("queue")? {
            let report: Option<DayReport> = read_json(&path)?;
            match report {
                Some(report)
                    if path == self.queue_path(report.day) && report.install == install =>
                {
                    queue.insert(report.day, report);
                }
                _ => write::remove_file(&path)?,
            }
        }
        Ok(queue)
    }

    /// Every tally whose file holds that week's tally, counted under `install`.
    pub(crate) fn read_tallies(&self, install: InstallId) -> io::Result<BTreeMap<Week, TallyFile>> {
        let mut tallies = BTreeMap::new();
        for path in self.list("tally")? {
            let tally: Option<TallyFile> = read_json(&path)?;
            match tally {
                Some(tally)
                    if path == self.tally_path(tally.week()) && tally.install() == install =>
                {
                    tallies.insert(tally.week(), tally);
                }
                _ => write::remove_file(&path)?,
            }
        }
        Ok(tallies)
    }

    /// The files in `<counts>/<folder>`, sorted; a missing folder has none. A leftover temp file
    /// is removed, and so is anything that is not a plain file (a folder, a link), which could
    /// otherwise fail every read.
    fn list(&self, folder: &str) -> io::Result<Vec<PathBuf>> {
        let entries = match std::fs::read_dir(self.root.join(folder)) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            let temp = path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'));
            if kind.is_dir() {
                write::remove_dir_all(&path)?;
            } else if temp || !kind.is_file() {
                // A link to a folder is removed as a folder on Windows.
                write::remove_file(&path).or_else(|_| write::remove_dir_all(&path))?;
            } else {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    }
}

/// The file at `path` as `T`: `None` when it is missing, and when it cannot be read as `T`
/// (or is a folder), in which case it is removed.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    remove_if_folder(path)?;
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    if let Ok(value) = serde_json::from_slice(&bytes) {
        return Ok(Some(value));
    }
    write::remove_file(path)?;
    Ok(None)
}

/// Removes a folder where a file should be, which would otherwise fail every read and write.
fn remove_if_folder(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => write::remove_dir_all(path),
        _ => Ok(()),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    write::replace(path, &bytes)
}
