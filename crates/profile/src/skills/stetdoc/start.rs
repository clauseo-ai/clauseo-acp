//! stetdoc at the app's start: what the agents get of it at once, and the tries that run in the
//! background when the start did not wait for an answer.
//!
//! The app waits for [`start`] before the host starts, and every command and sign-in wait for
//! the host. A new, unsigned program's first run can take seconds while the OS scans it
//! (Defender or Smart App Control on Windows, after an install or an update, or again after a
//! signature update), and it answers in milliseconds afterwards. So:
//!
//! - a stetdoc already seen to answer (the same file, by path, size and modification time, as
//!   the marker [`seen`] keeps under the app's data says) gets one try within [`Limits::seen`]
//!   (3 s) at the start, `--version` and the skill together;
//! - one not seen yet (a first run after an install or an update) gets no try at the start: the
//!   host starts at once.
//!
//! Either way, a stetdoc the start has no answer from (not seen, or its try timed out or did not
//! start) stays on at once, with the skill a previous run cached when one loads, and
//! [`recheck`] runs the same two steps on a thread of its own, one try after another while
//! they time out or do not start: [`Limits::unseen`] (20 s, not seen yet only), then
//! [`Limits::retry`] (20 s), then [`Limits::recheck`] (30 s). A stetdoc that hangs is killed
//! at each limit. What the first try with an answer found goes into the profile for the agent
//! starts that follow:
//!
//! - it works: stetdoc stays on, with the skill it loaded;
//! - `--version` fails for real (an error exit, no version), or the last try did not start:
//!   stetdoc is treated as absent;
//! - the last try passes its limit too, or only the skill fails: nothing changes.
//!
//! An answer, at the start or in the background, is recorded in the marker, so the next start
//! gives the stetdoc 3 s. A `--version` that fails for real at the start is absent at once, and
//! nothing is checked again. The functions take the launcher and the timer, so tests drive them
//! with the stub stetdoc and timers the test controls.

use std::cell::Cell;
use std::path::Path;
use std::pin::pin;
use std::time::Duration;

use clauseo_process::{Launcher, Timer};
use futures::future::{Either, select};

use super::{RunError, SKILL_FILE, STETDOC, load_skill_at, skill_source, version_within};
use crate::skills::{SkillSource, is_current, manifest::Manifest};

pub mod seen;

/// The target of the start's support lines: host.log keeps info only from `clauseo_*::lifecycle`
/// targets.
const LIFECYCLE: &str = "clauseo_profile::stetdoc::lifecycle";

/// How long the try at start-up may take, both runs together (`--version`, then `skills get
/// core` when the cached skill is stale), for a stetdoc already seen to answer. The host starts
/// after it, and every command waits for the host, so this bounds how long a slow stetdoc
/// delays the app.
pub const FIRST_LIMIT: Duration = Duration::from_secs(3);

/// How long the first background try may take for a stetdoc not seen to answer yet, and the
/// next try after one that timed out or did not start.
pub const LONG_LIMIT: Duration = Duration::from_secs(20);

/// How long the last background try may take, both runs together.
pub const RECHECK_LIMIT: Duration = Duration::from_secs(30);

/// The limits of the start's tries: [`Self::APP`] in the app; tests pass their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The try at the start, for a stetdoc already seen to answer.
    pub seen: Duration,
    /// The first background try, for a stetdoc not seen to answer yet.
    pub unseen: Duration,
    /// The background try after one that timed out or did not start.
    pub retry: Duration,
    /// The last background try.
    pub recheck: Duration,
}

impl Limits {
    /// The app's: 3 s at the start for a stetdoc seen before; in the background 20 s for one
    /// not seen yet, 20 s for the retry, then 30 s.
    pub const APP: Self = Self {
        seen: FIRST_LIMIT,
        unseen: LONG_LIMIT,
        retry: LONG_LIMIT,
        recheck: RECHECK_LIMIT,
    };
}

/// What one [`check`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// `--version` worked and the skill loaded.
    Skill(SkillSource),
    /// `--version` worked but the skill did not load (`skills get core` failed or took too
    /// long, or its text was refused).
    NoSkill,
    /// `--version` failed for real: it exited with an error, or printed no version.
    Broken,
    /// `--version` could not be started (missing, not a program, refused, or held by the OS).
    NotStarted,
    /// `--version` passed the limit: stetdoc may work but be slow.
    TimedOut,
}

impl Found {
    /// Whether `--version` answered, with the skill or without.
    #[must_use]
    pub fn answered(&self) -> bool {
        matches!(self, Self::Skill(_) | Self::NoSkill)
    }

    /// Whether a try that found this gets the next one: it passed the limit or did not start,
    /// either of which a scanning OS can cause.
    #[must_use]
    pub fn retried(&self) -> bool {
        matches!(self, Self::TimedOut | Self::NotStarted)
    }
}

/// What the agents get of stetdoc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// stetdoc is on: its folder on the agents' `PATH`, the instructions' stetdoc line, the Word
    /// blackline; its skill when one loaded.
    On(Option<SkillSource>),
    /// stetdoc is treated as absent: no `PATH` entry, no stetdoc line, no skill.
    Off,
}

impl State {
    /// Whether stetdoc is on.
    #[must_use]
    pub fn is_on(&self) -> bool {
        matches!(self, Self::On(_))
    }

    /// The skill, when stetdoc is on and one loaded.
    #[must_use]
    pub fn skill(&self) -> Option<&SkillSource> {
        match self {
            Self::On(skill) => skill.as_ref(),
            Self::Off => None,
        }
    }
}

/// What [`start`] decided: the state the agents start with, and the background tries'
/// limits, in order (none when the start has its answer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    /// The state from now on, until a [`recheck`] changes it.
    pub state: State,
    /// The limits of the tries [`recheck`] runs, one after another while they time out or do
    /// not start.
    pub later: Vec<Duration>,
}

/// stetdoc at the start, with `cached` as the skills folder a previous run wrote and `seen` as
/// the marker of the stetdoc last seen to answer: one try within [`Limits::seen`] when `seen`
/// names `program`, none otherwise. Starts stetdoc and reads and writes files: off the host
/// thread.
pub async fn start<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    program: &Path,
    cached: &Path,
    seen: &Path,
    limits: Limits,
) -> Start {
    if !seen::is_seen(seen, program) {
        let skill = cached_skill(cached);
        tracing::info!(
            target: LIFECYCLE,
            cached_skill = skill.is_some(),
            "stetdoc has not been seen to answer yet (a new install or update); it stays on and \
             is tried in the background"
        );
        return Start {
            state: State::On(skill),
            later: vec![limits.unseen, limits.retry, limits.recheck],
        };
    }
    let found = check(launcher, timer, program, cached, limits.seen).await;
    first(found, limits, || cached_skill(cached))
}

/// What the agents start with after the try at the start found `found`; `cached` gives the
/// skill a previous run cached, read only when the background tries follow.
pub fn first(found: Found, limits: Limits, cached: impl FnOnce() -> Option<SkillSource>) -> Start {
    let (state, later) = match found {
        Found::Skill(skill) => (State::On(Some(skill)), Vec::new()),
        Found::NoSkill => (State::On(None), Vec::new()),
        Found::Broken => (State::Off, Vec::new()),
        Found::TimedOut | Found::NotStarted => {
            let skill = cached();
            tracing::warn!(
                target: LIFECYCLE,
                timed_out = found == Found::TimedOut,
                limit_secs = limits.seen.as_secs(),
                cached_skill = skill.is_some(),
                "stetdoc did not answer at the start (the OS may be scanning it); it stays on \
                 and is tried again in the background"
            );
            (State::On(skill), vec![limits.retry, limits.recheck])
        }
    };
    Start { state, later }
}

/// The background tries, within `later`'s limits one after another while they time out or do
/// not start: the state to put in place of `current`, or `None` to keep it. Records an answer
/// in `seen`. Starts stetdoc and reads and writes files: on a thread of its own.
pub async fn recheck<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    program: &Path,
    cached: &Path,
    seen: &Path,
    current: &State,
    later: &[Duration],
) -> Option<State> {
    for (at, &limit) in later.iter().enumerate() {
        let found = check(launcher, timer, program, cached, limit).await;
        if found.answered() {
            seen::record(seen, program);
        }
        if found.retried() && at + 1 < later.len() {
            tracing::warn!(
                target: LIFECYCLE,
                timed_out = found == Found::TimedOut,
                limit_secs = limit.as_secs(),
                "stetdoc did not answer in the background; it is tried once more"
            );
            continue;
        }
        return after(current, found, limit);
    }
    None
}

/// What a background check within `limit` that found `found` does to `current`: the new
/// state, or `None`.
pub fn after(current: &State, found: Found, limit: Duration) -> Option<State> {
    match found {
        Found::Skill(skill) => {
            tracing::info!(
                target: LIFECYCLE,
                outcome = "skill",
                "stetdoc answered when checked again; its skill goes to the next agent start"
            );
            Some(State::On(Some(skill)))
        }
        Found::NoSkill => {
            tracing::info!(
                target: LIFECYCLE,
                outcome = "no_skill",
                "stetdoc answered when checked again, without its skill; it stays on"
            );
            None
        }
        Found::Broken | Found::NotStarted => {
            tracing::warn!(
                target: LIFECYCLE,
                outcome = "broken",
                "stetdoc failed when checked again; it is treated as absent from the next \
                 agent start"
            );
            Some(State::Off)
        }
        Found::TimedOut => {
            tracing::warn!(
                target: LIFECYCLE,
                outcome = "timed_out",
                limit_secs = limit.as_secs(),
                cached_skill = current.skill().is_some(),
                "stetdoc took too long again; it stays on with no fresh skill"
            );
            None
        }
    }
}

/// `--version`, then the skill ([`load_skill_at`]), both within `limit` together. On the
/// deadline the run is dropped, and with it its process handle, which kills stetdoc's tree. A
/// failed `--version` and a failed skill are logged as warnings here; a timeout is the
/// caller's to log.
pub async fn check<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    program: &Path,
    cached: &Path,
    limit: Duration,
) -> Found {
    // Set once `--version` worked, so a deadline tells which run it stopped.
    let version_worked = Cell::new(false);
    let work = async {
        let version = version_within(launcher, timer, program, limit).await?;
        version_worked.set(true);
        Ok::<_, RunError>(load_skill_at(launcher, timer, program, &version, cached).await)
    };
    let done = {
        let work = pin!(work);
        let deadline = pin!(timer.after(limit));
        match select(work, deadline).await {
            Either::Left((done, _)) => Some(done),
            Either::Right(((), _)) => None,
        }
    };
    match done {
        Some(Ok(Ok(skill))) => Found::Skill(skill),
        Some(Ok(Err(err))) => {
            tracing::warn!(%err, "stetdoc's skill text is left out; stetdoc itself stays on");
            Found::NoSkill
        }
        Some(Err(RunError::Failed(err))) => {
            tracing::warn!(%err, "stetdoc --version failed; stetdoc is treated as absent");
            Found::Broken
        }
        Some(Err(RunError::NotStarted(err))) => {
            tracing::warn!(%err, "stetdoc --version did not start");
            Found::NotStarted
        }
        None if version_worked.get() => {
            tracing::warn!(
                limit_secs = limit.as_secs(),
                "stetdoc's skill text took too long and is left out; stetdoc itself stays on"
            );
            Found::NoSkill
        }
        Some(Err(RunError::TimedOut(_))) | None => Found::TimedOut,
    }
}

/// The skill a previous run cached in `cached` (the skills folder the launch writes fill), at
/// the version its manifest records, when its files are unchanged and its text loads. It may
/// be an older stetdoc's, after an update; the background check then puts the current one in
/// place. Reads files.
pub fn cached_skill(cached: &Path) -> Option<SkillSource> {
    let entry = Manifest::read(cached).ok()?.entry(STETDOC)?;
    if !is_current(cached, STETDOC, &entry.version).ok()? {
        return None;
    }
    let text = std::fs::read(cached.join(STETDOC).join(SKILL_FILE)).ok()?;
    skill_source(&entry.version, &text).ok()
}
