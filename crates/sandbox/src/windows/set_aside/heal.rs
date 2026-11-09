//! The `SYSTEM` side's heal of a lawyer's drive folder another account made first
//! (`<volume root>\.clauseo-set-aside-<lawyer's SID>`, [`super::drive_folder_name`]), which a
//! launch refuses and never takes back ([`super::SetAsideFailure::DriveFolderNotYours`]). The
//! refresh task runs it as `SYSTEM` (`refresh::windows_refresher::run`), for each lawyer the
//! machine's record names and each local drive, so nothing the lawyer or a squatter writes
//! chooses what it touches beyond that one name per drive.
//!
//! The heal never opens, lists or moves what the folder holds. It opens the folder itself
//! ([`Healer::open`]: `FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS` on NTFS), and
//! on that one handle: refuses a reparse point or a file ([`Healer::facts`]); leaves one the
//! lawyer owns; else makes `SYSTEM` its owner with a protected DACL of `SYSTEM` and
//! `Administrators` alone ([`Healer::lock_down`]), and renames it in place to
//! `<name>.squatted-<UTC second>-<8 hex>` ([`Healer::rename`], [`super::SQUATTED`]). Then it
//! makes the lawyer's folder afresh, owned by the lawyer with the drive DACL
//! ([`Healer::make_fresh`]). Each heal or refusal writes one log line, naming the drive, the
//! lawyer's SID and the owner found, nothing inside.
//!
//! Nothing a squatter does can make it hang or repeat without end:
//! - the 8 hex digits are random, so a squatter can't make the name first; a name taken anyway is
//!   tried again with new digits, [`NAME_TRIES`] names in all;
//! - a rename a handle or a lease holds up is tried [`BUSY_TRIES`] times, [`BUSY_PAUSE`] apart,
//!   each call bounded by [`CALL_WAIT`] on NTFS (2 s in all), and then given up, logged, the
//!   folder left locked down for the next run;
//! - a folder made again at the lawyer's name between the rename and the fresh folder is counted
//!   and logged, and healed again, [`ROUNDS`] rounds a run at most.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::time::second_stamp;
use super::{SQUATTED, drive_folder_name};
use crate::windows::sids::Sid;

/// What the heal reads from a drive folder's own handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    /// Whether it is a reparse point (a junction, a symbolic link, any other tag).
    pub(crate) reparse: bool,
    /// Whether it is a folder.
    pub(crate) folder: bool,
    /// Its owner's SID, as text.
    pub(crate) owner: String,
}

/// The file-system calls the heal makes: NTFS as `SYSTEM` (`private_dir/drive/heal.rs`), and
/// the tests' fake. None takes a path below the drive folder: nothing inside is ever opened.
pub(crate) trait Healer {
    /// The drive folder, opened as itself.
    type Opened;

    /// Opens `path` itself, never what a link there names; `None` when nothing is there.
    fn open(&self, path: &Path) -> io::Result<Option<Self::Opened>>;

    /// What `opened` is, read from its handle.
    fn facts(&self, opened: &Self::Opened) -> io::Result<Facts>;

    /// Makes `SYSTEM` the owner of `opened` and gives it a protected DACL of `SYSTEM` and
    /// `Administrators`, on its handle alone: nothing inside is changed or walked.
    fn lock_down(&self, opened: &Self::Opened) -> io::Result<()>;

    /// Renames `opened` by its handle to `name` in the folder it is in, never replacing: a name
    /// taken is `AlreadyExists`, a rename a handle or a lease holds up `ResourceBusy`, one
    /// that takes longer than [`CALL_WAIT`] `TimedOut`.
    fn rename(&self, opened: &Self::Opened, name: &OsStr) -> io::Result<()>;

    /// Makes the folder `path` (nothing may be there) owned by `lawyer`, with the drive DACL.
    fn make_fresh(&self, path: &Path, lawyer: &Sid) -> io::Result<()>;

    /// Writes `line` to the log (the Event Log on Windows), as an error when `error`.
    fn log(&self, error: bool, line: &str);

    /// A random number, for the set-aside name's digits.
    fn random(&self) -> io::Result<u32>;

    /// Waits `wait` before a rename is tried again.
    fn pause(&self, wait: Duration);
}

/// The most names a set-aside is tried under, each with new random digits.
pub(crate) const NAME_TRIES: usize = 4;
/// The most times a rename held up by a handle or a lease is tried.
pub(crate) const BUSY_TRIES: usize = 3;
/// The pause between two tries of a rename held up.
pub(crate) const BUSY_PAUSE: Duration = Duration::from_millis(250);
/// The longest one open or rename may take on NTFS before the heal gives it up: with
/// [`BUSY_TRIES`] and [`BUSY_PAUSE`], 2 s for a folder's rename in all.
pub(crate) const CALL_WAIT: Duration = Duration::from_millis(500);
/// The most rounds of one folder's heal in a run: each after a folder made again at the
/// lawyer's name before the fresh one.
pub(crate) const ROUNDS: usize = 3;

/// What one heal did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Healed {
    /// Nothing is there: the lawyer's next launch makes it.
    Missing,
    /// The lawyer owns it: left as it is.
    Lawyers,
    /// Set aside under this name, and the lawyer's made afresh.
    SetAside(String),
    /// Set aside under this name, but the fresh one couldn't be made (something is there again,
    /// say): the next launch makes it, or is refused again.
    SetAsideOnly(String),
    /// A reparse point or a file: left untouched.
    Refused,
    /// A call failed, with this kind, before anything was renamed.
    Failed(io::ErrorKind),
}

/// Heals `lawyer`'s drive folder on each of `roots` (volume roots): [`heal`] for each.
pub(crate) fn heal_all(
    healer: &impl Healer,
    roots: &[PathBuf],
    lawyers: &[Sid],
    now: SystemTime,
) -> Vec<Healed> {
    let mut healed = Vec::new();
    for root in roots {
        for lawyer in lawyers {
            healed.push(heal(healer, root, lawyer, now));
        }
    }
    healed
}

/// Heals `lawyer`'s drive folder at the volume root `root`, as the module says. Logs one line
/// for anything other than [`Healed::Missing`] and [`Healed::Lawyers`], and one more for each
/// folder made again before the fresh one.
pub(crate) fn heal(healer: &impl Healer, root: &Path, lawyer: &Sid, now: SystemTime) -> Healed {
    let mut last: Option<String> = None;
    for round in 1..=ROUNDS {
        let aside = match heal_once(healer, root, lawyer, now) {
            Round::Done(healed) => {
                return match (healed, last) {
                    // Gone, or the lawyer's by now, after a round set one aside.
                    (Healed::Missing, Some(aside)) => Healed::SetAsideOnly(aside),
                    (Healed::Lawyers, Some(aside)) => Healed::SetAside(aside),
                    (healed, _) => healed,
                };
            }
            Round::Again(aside) => aside,
        };
        let drive = root.display();
        let line = format!(
            "the drive folder on {drive} for {lawyer} was made again by another account after it \
             was set aside as {aside}, before the lawyer's ({round} of {ROUNDS})"
        );
        healer.log(true, &line);
        last = Some(aside);
    }
    let drive = root.display();
    let line = format!(
        "the drive folder on {drive} for {lawyer} was made again {ROUNDS} times this run; the next \
         run heals it again"
    );
    healer.log(true, &line);
    Healed::SetAsideOnly(last.unwrap_or_default())
}

/// How one round of a heal ended.
enum Round {
    /// Done, with this outcome.
    Done(Healed),
    /// Set aside under this name, and something was at the lawyer's name again before the
    /// fresh folder: another round.
    Again(String),
}

/// One round of [`heal`].
fn heal_once(healer: &impl Healer, root: &Path, lawyer: &Sid, now: SystemTime) -> Round {
    let name = drive_folder_name(lawyer.as_str());
    let path = root.join(&name);
    let drive = root.display();
    let failed = |what: &str, error: &io::Error| {
        let kind = error.kind();
        let line = format!("the drive folder on {drive} for {lawyer} {what} ({kind}); kept");
        healer.log(true, &line);
        Round::Done(Healed::Failed(kind))
    };
    let opened = match healer.open(&path) {
        Ok(Some(opened)) => opened,
        Ok(None) => return Round::Done(Healed::Missing),
        Err(error) => return failed("can't be opened", &error),
    };
    let facts = match healer.facts(&opened) {
        Ok(facts) => facts,
        Err(error) => return failed("can't be read", &error),
    };
    if facts.reparse || !facts.folder {
        let owner = &facts.owner;
        let line = format!(
            "the drive folder on {drive} for {lawyer} is a link or a file ({owner} owns it); \
             left as it is"
        );
        healer.log(true, &line);
        return Round::Done(Healed::Refused);
    }
    if facts.owner.eq_ignore_ascii_case(lawyer.as_str()) {
        return Round::Done(Healed::Lawyers);
    }
    if let Err(error) = healer.lock_down(&opened) {
        return failed("can't be locked down", &error);
    }
    let aside = match rename_aside(healer, &opened, &name, now) {
        Ok(aside) => aside,
        Err(error) => {
            return failed(
                "can't be renamed, so it is left locked down for the next run",
                &error,
            );
        }
    };
    drop(opened);
    let owner = &facts.owner;
    match healer.make_fresh(&path, lawyer) {
        Ok(()) => {
            let line = format!(
                "the drive folder on {drive} for {lawyer}, which {owner} owned, was set aside as \
                 {aside}; the lawyer's was made afresh"
            );
            healer.log(false, &line);
            Round::Done(Healed::SetAside(aside))
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Round::Again(aside),
        Err(error) => {
            let kind = error.kind();
            let line = format!(
                "the drive folder on {drive} for {lawyer}, which {owner} owned, was set aside as \
                 {aside}; the lawyer's couldn't be made afresh ({kind})"
            );
            healer.log(true, &line);
            Round::Done(Healed::SetAsideOnly(aside))
        }
    }
}

/// Renames `opened` to `<name>.squatted-<UTC second>-<8 hex>`, as the module says: a name taken
/// tried again with new digits, a rename held up tried again after a pause, each a bounded
/// number of times. Returns the name it got, or the last error.
fn rename_aside<H: Healer>(
    healer: &H,
    opened: &H::Opened,
    name: &str,
    now: SystemTime,
) -> io::Result<String> {
    let (mut names, mut busy) = (1, 1);
    loop {
        let digits = healer.random()?;
        let aside = format!("{name}{SQUATTED}{}-{digits:08x}", second_stamp(now));
        match healer.rename(opened, OsStr::new(&aside)) {
            Ok(()) => return Ok(aside),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && names < NAME_TRIES => {
                names += 1;
            }
            Err(error) if error.kind() == io::ErrorKind::ResourceBusy && busy < BUSY_TRIES => {
                busy += 1;
                healer.pause(BUSY_PAUSE);
            }
            Err(error) => return Err(error),
        }
    }
}
