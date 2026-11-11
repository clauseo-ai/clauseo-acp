//! stetdoc at the app's start: what the agents get of it, and one more check in the background
//! when its first run was only slow.
//!
//! The app waits for [`start`] before the host starts, so it gets [`FIRST_LIMIT`] for
//! `--version` and the skill together. A new, unsigned program's first run can take longer
//! while the OS scans it (Defender or Smart App Control on Windows, after an install or an
//! update), and it answers in milliseconds afterwards. So a `--version` that only passed the
//! limit keeps stetdoc on at once, with the skill a previous run cached when one loads, and
//! [`recheck`] runs the same two steps again, off the start, within [`RECHECK_LIMIT`]:
//!
//! - it works: stetdoc stays on, with the skill it loaded, from the next agent start;
//! - `--version` fails for real (an error exit, no version, no start): stetdoc is treated as
//!   absent for the agent starts that follow;
//! - it passes the limit again, or only the skill fails: nothing changes.
//!
//! A `--version` that fails for real within [`FIRST_LIMIT`] is absent at once, and nothing is
//! checked again. The functions take the launcher and the timer, so tests drive them with the
//! stub stetdoc and timers that end at once or never.

use std::cell::Cell;
use std::path::Path;
use std::pin::pin;
use std::time::Duration;

use clauseo_process::{Launcher, Timer};
use futures::future::{Either, select};

use super::{RunError, SKILL_FILE, STETDOC, load_skill_at, skill_source, version_within};
use crate::skills::{SkillSource, is_current, manifest::Manifest};

/// The target of the start's support lines: host.log keeps info only from `clauseo_*::lifecycle`
/// targets.
const LIFECYCLE: &str = "clauseo_profile::stetdoc::lifecycle";

/// How long the first check at start-up may take, both runs together (`--version`, then
/// `skills get core` when the cached skill is stale). The host starts after it, and every
/// command waits for the host, so this bounds how long a slow stetdoc delays the app.
pub const FIRST_LIMIT: Duration = Duration::from_secs(3);

/// How long the background check after a slow first run may take, both runs together.
pub const RECHECK_LIMIT: Duration = Duration::from_secs(30);

/// What one [`check`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// `--version` worked and the skill loaded.
    Skill(SkillSource),
    /// `--version` worked but the skill did not load (`skills get core` failed or took too
    /// long, or its text was refused).
    NoSkill,
    /// `--version` failed for real: it did not start, exited with an error, or printed no
    /// version.
    Broken,
    /// `--version` passed the limit: stetdoc may work but be slow.
    TimedOut,
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

/// What [`start`] decided: the state the agents start with, and whether [`recheck`] should run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    /// The state from now on, until a [`recheck`] changes it.
    pub state: State,
    /// Whether the first `--version` only passed its limit, so the check runs again.
    pub recheck: bool,
}

/// The first check, within [`FIRST_LIMIT`], of `program` with `cached` as the skills folder a
/// previous run wrote. Starts stetdoc and reads files: off the host thread.
pub async fn start<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    program: &Path,
    cached: &Path,
) -> Start {
    let found = check(launcher, timer, program, cached, FIRST_LIMIT).await;
    first(found, || cached_skill(cached))
}

/// What the agents start with after a first check that found `found`; `cached` gives the
/// skill a previous run cached, read only after a timeout.
pub fn first(found: Found, cached: impl FnOnce() -> Option<SkillSource>) -> Start {
    let (state, recheck) = match found {
        Found::Skill(skill) => (State::On(Some(skill)), false),
        Found::NoSkill => (State::On(None), false),
        Found::Broken => (State::Off, false),
        Found::TimedOut => {
            let skill = cached();
            tracing::warn!(
                target: LIFECYCLE,
                limit_secs = FIRST_LIMIT.as_secs(),
                cached_skill = skill.is_some(),
                "stetdoc took too long on its first run; it stays on and is checked again \
                 in the background"
            );
            (State::On(skill), true)
        }
    };
    Start { state, recheck }
}

/// The check after a slow first run, within [`RECHECK_LIMIT`]: the state to put in place of
/// `current`, or `None` to keep it. Starts stetdoc and reads files: on a thread of its own.
pub async fn recheck<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    program: &Path,
    cached: &Path,
    current: &State,
) -> Option<State> {
    let found = check(launcher, timer, program, cached, RECHECK_LIMIT).await;
    after(current, found)
}

/// What a background check that found `found` does to `current`: the new state, or `None`.
pub fn after(current: &State, found: Found) -> Option<State> {
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
        Found::Broken => {
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
                limit_secs = RECHECK_LIMIT.as_secs(),
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
