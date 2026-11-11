//! stetdoc at the app's start (`skills::stetdoc::start`): a `--version` that only passed the
//! first limit keeps stetdoc on at once (with the cached skill when one loads) and is checked
//! again in the background, whose answer the next agent start gets; a real failure within the
//! first limit is absent at once and never checked again. The stub stetdoc runs for real. A slow
//! stetdoc is the stub held by `stub-hang` with a timer whose waits end at once: the run cannot
//! answer, so the deadline wins whichever future `select` polls first. A stetdoc that answers in
//! time is the stub with a timer whose waits never end. No outcome hangs on the wall clock.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use clauseo_process::{PassthroughLauncher, SpawnOptions, Timer};
use clauseo_profile::instructions::STETDOC_CLI;
use clauseo_profile::skills::stetdoc::start::{
    self, FIRST_LIMIT, Found, RECHECK_LIMIT, Start, State, cached_skill,
};
use clauseo_profile::skills::stetdoc::{STETDOC, skill_source};
use clauseo_profile::skills::{self, render, write_rendered};
use clauseo_profile::{ClauseoProfile, SkillSource, write_skills};
use clauseo_protocol::AgentKind;
use futures::channel::oneshot;
use futures::executor::block_on;

use super::{Scratch, profile_paths, stub, write};

/// What `stetdoc skills get core` printed, byte for byte, from stetdoc 0.9.3 (macOS arm64).
const REAL_0_9_3: &[u8] = include_bytes!("../fixtures/stetdoc-0.9.3-skills-get-core.md");

/// A timer whose waits end at once: every limit has passed. With the stub held ([`slow`]), as
/// for a stetdoc that the OS holds while it scans the program; a stub free to answer could
/// finish before the first poll and beat it.
struct Passed;

impl Timer for Passed {
    fn after(&self, _duration: Duration) -> impl Future<Output = ()> + Send {
        std::future::ready(())
    }
}

/// A timer whose waits never end: stetdoc answers in time.
struct Never;

impl Timer for Never {
    fn after(&self, _duration: Duration) -> impl Future<Output = ()> + Send {
        std::future::pending()
    }
}

/// A timer for one [`start::check`] whose skill run hangs: its first wait, the check's own
/// deadline, ends once its third begins, which is the limit of `skills get core`, so after
/// `--version` answered and the skill run started; every other wait never ends.
#[cfg(test)]
struct AtTheSkillRun {
    calls: AtomicUsize,
    fire: Mutex<Option<oneshot::Sender<()>>>,
    fired: Mutex<Option<oneshot::Receiver<()>>>,
}

#[cfg(test)]
impl AtTheSkillRun {
    fn new() -> Self {
        let (fire, fired) = oneshot::channel();
        Self {
            calls: AtomicUsize::new(0),
            fire: Mutex::new(Some(fire)),
            fired: Mutex::new(Some(fired)),
        }
    }
}

#[cfg(test)]
impl Timer for AtTheSkillRun {
    fn after(&self, _duration: Duration) -> impl Future<Output = ()> + Send {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 2
            && let Some(fire) = self.fire.lock().unwrap().take()
        {
            let _sent = fire.send(());
        }
        let fired = if call == 0 {
            self.fired.lock().unwrap().take()
        } else {
            None
        };
        async move {
            match fired {
                Some(fired) => {
                    let _ended = fired.await;
                }
                None => std::future::pending().await,
            }
        }
    }
}

/// A stub stetdoc in `<root>/bin` printing `version` and 0.9.3's skill text, and the skills
/// folder of Claude's plugin, which the launch writes fill and the start reads as its cache.
#[cfg(test)]
fn stetdoc_in(root: &Scratch, version: &str) -> (PathBuf, PathBuf) {
    let bin = root.join("bin");
    write::fresh_dir(&bin).unwrap();
    let real = std::str::from_utf8(REAL_0_9_3).unwrap();
    (
        stub(&bin, version, real),
        profile_paths(root).claude_skills(),
    )
}

/// Holds every run of the stub in `root` until [`answers`]: it prints nothing and does not exit.
#[cfg(test)]
fn slow(root: &Scratch) {
    write::write(&root.join("bin").join("stub-hang"), b"").unwrap();
}

/// Lets the stub in `root` answer again.
#[cfg(test)]
fn answers(root: &Scratch) {
    write::remove_file(&root.join("bin").join("stub-hang")).unwrap();
}

/// 0.9.3's skill as it loads.
#[cfg(test)]
fn skill_0_9_3() -> SkillSource {
    skill_source("stetdoc 0.9.3", REAL_0_9_3).unwrap()
}

/// [`start::start`] with `timer`.
#[cfg(test)]
fn first<T: Timer>(timer: &T, program: &Path, cached: &Path) -> Start {
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    block_on(start::start(&launcher, timer, program, cached))
}

/// [`start::recheck`] with `timer`.
#[cfg(test)]
fn again<T: Timer>(timer: &T, program: &Path, cached: &Path, now: &State) -> Option<State> {
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    block_on(start::recheck(&launcher, timer, program, cached, now))
}

/// What the agents are told and given under `state`: whether the instructions hold the
/// stetdoc line, and stetdoc's `SKILL.md` text when the profile renders one.
#[cfg(test)]
fn told(root: &Scratch, state: &State) -> (bool, Option<String>) {
    let (profile, refused) =
        ClauseoProfile::new(profile_paths(root), None).with_stetdoc(state, false);
    assert!(refused.is_empty(), "{refused:?}");
    let line = profile.instructions().contains(STETDOC_CLI.trim());
    let write = profile.skill_files(Some(AgentKind::Claude)).unwrap();
    let skill = write
        .rendered()
        .skills()
        .iter()
        .find(|skill| skill.name() == STETDOC)
        .map(|skill| String::from_utf8(skill.files()[0].bytes.clone()).unwrap());
    (line, skill)
}

/// A Claude launch's skills write under `state`, as the host runs it before the agent starts.
#[cfg(test)]
fn launch_write(root: &Scratch, state: &State) {
    let (profile, _) = ClauseoProfile::new(profile_paths(root), None).with_stetdoc(state, false);
    write_skills(&profile.launch_writes(Some(AgentKind::Claude))[0]).unwrap();
}

/// An older stetdoc's skill in `cached`, as an earlier start's launch write left it.
#[cfg(test)]
fn cache_old_skill(cached: &Path) -> SkillSource {
    let old = skill_source("stetdoc 0.9.2", b"---\ndescription: Old.\n---\nold body\n").unwrap();
    write_rendered(cached, &render(std::slice::from_ref(&old)).unwrap()).unwrap();
    old
}

#[test]
fn the_limits_are_3_s_at_start_and_30_s_in_the_background() {
    assert_eq!(FIRST_LIMIT, Duration::from_secs(3));
    assert_eq!(RECHECK_LIMIT, Duration::from_secs(30));
}

/// A timeout with no cached skill: on at once, the instructions' stetdoc line included, with no
/// skill; the background check that works brings the skill to the next agent start.
#[test]
fn a_timeout_then_a_background_success_brings_the_skill() {
    let root = Scratch::new("start-timeout-success");
    let (program, cached) = stetdoc_in(&root, "stetdoc 0.9.3\n");
    slow(&root);
    let started = first(&Passed, &program, &cached);
    assert_eq!(
        started,
        Start {
            state: State::On(None),
            recheck: true
        }
    );
    assert_eq!(told(&root, &started.state), (true, None));

    answers(&root);
    let next = again(&Never, &program, &cached, &started.state);
    assert_eq!(next, Some(State::On(Some(skill_0_9_3()))));
    let (line, skill) = told(&root, &next.unwrap());
    assert!(line);
    let real = std::str::from_utf8(REAL_0_9_3).unwrap();
    assert!(skill.unwrap().ends_with(real));
}

/// A timeout, then a background `--version` that fails for real (an error exit, no version):
/// on at once, then absent: no stetdoc line, no skill.
#[test]
fn a_timeout_then_a_background_failure_turns_stetdoc_off() {
    for (at, version) in ["", "\n"].into_iter().enumerate() {
        let root = Scratch::new(&format!("start-timeout-broken-{at}"));
        let (program, cached) = stetdoc_in(&root, "stetdoc 0.9.3\n");
        slow(&root);
        let started = first(&Passed, &program, &cached);
        assert!(started.recheck, "case {at}");
        assert_eq!(told(&root, &started.state), (true, None), "case {at}");
        if version.is_empty() {
            // The stub with no `stub-version.txt` beside it exits 4.
            write::remove_file(&root.join("bin").join("stub-version.txt")).unwrap();
        } else {
            write::write(
                &root.join("bin").join("stub-version.txt"),
                version.as_bytes(),
            )
            .unwrap();
        }
        answers(&root);
        let next = again(&Never, &program, &cached, &started.state);
        assert_eq!(next, Some(State::Off), "case {at}");
        assert_eq!(told(&root, &State::Off), (false, None), "case {at}");
    }
}

/// A timeout with a cached skill (an older stetdoc's, as after an update): on at once with that
/// skill; the background check puts the current one in its place, and the launch write that
/// follows makes the cache current for it.
#[test]
fn a_timeout_uses_the_cached_skill_and_the_background_refreshes_it() {
    let root = Scratch::new("start-timeout-cached");
    let (program, cached) = stetdoc_in(&root, "stetdoc 0.9.3\n");
    let old = cache_old_skill(&cached);
    assert_eq!(cached_skill(&cached), Some(old.clone()));

    slow(&root);
    let started = first(&Passed, &program, &cached);
    assert_eq!(
        started,
        Start {
            state: State::On(Some(old)),
            recheck: true
        }
    );
    let (line, skill) = told(&root, &started.state);
    assert!(line);
    assert!(skill.unwrap().ends_with("old body\n"));

    answers(&root);
    let next = again(&Never, &program, &cached, &started.state).unwrap();
    assert_eq!(next, State::On(Some(skill_0_9_3())));
    let fresh = next.skill().unwrap().clone();
    write_rendered(&cached, &render(&[fresh]).unwrap()).unwrap();
    assert!(skills::is_current(&cached, STETDOC, "stetdoc 0.9.3").unwrap());
    assert_eq!(cached_skill(&cached), Some(skill_0_9_3()));
}

/// A cached skill whose file drifted from its manifest is not used.
#[test]
fn a_drifted_cached_skill_is_not_used() {
    let root = Scratch::new("start-cached-drift");
    let cached = root.join("skills");
    write_rendered(&cached, &render(&[skill_0_9_3()]).unwrap()).unwrap();
    write::write(&cached.join(STETDOC).join("SKILL.md"), b"changed\n").unwrap();
    assert_eq!(cached_skill(&cached), None);
    assert_eq!(cached_skill(&root.join("none")), None);
}

/// A second timeout in the background leaves stetdoc on as it is.
#[test]
fn a_second_timeout_leaves_stetdoc_on() {
    let root = Scratch::new("start-timeout-twice");
    let (program, cached) = stetdoc_in(&root, "stetdoc 0.9.3\n");
    slow(&root);
    let started = first(&Passed, &program, &cached);
    assert_eq!(again(&Passed, &program, &cached, &started.state), None);
    assert_eq!(told(&root, &started.state), (true, None));
}

/// A `--version` that fails for real within the first limit is absent at once, and is not
/// checked again.
#[test]
fn a_real_failure_at_start_is_absent_and_not_checked_again() {
    let root = Scratch::new("start-broken");
    let (program, cached) = stetdoc_in(&root, "\n");
    assert_eq!(
        first(&Never, &program, &cached),
        Start {
            state: State::Off,
            recheck: false
        }
    );
    write::remove_file(&root.join("bin").join("stub-version.txt")).unwrap();
    assert_eq!(
        first(&Never, &program, &cached),
        Start {
            state: State::Off,
            recheck: false
        }
    );
    assert_eq!(told(&root, &State::Off), (false, None));
}

/// As before: a stetdoc that answers in time is on with its skill, and one whose skill alone
/// fails is on without it; neither is checked again.
#[test]
fn a_stetdoc_in_time_is_on_with_its_skill_or_without() {
    let root = Scratch::new("start-in-time");
    let (program, cached) = stetdoc_in(&root, "stetdoc 0.9.3\n");
    let started = first(&Never, &program, &cached);
    assert_eq!(
        started,
        Start {
            state: State::On(Some(skill_0_9_3())),
            recheck: false
        }
    );

    let refused = "---\nname: stetdoc\n---\n# no description\n";
    write::write(&root.join("bin").join("stub-skill.md"), refused.as_bytes()).unwrap();
    let started = first(&Never, &program, &cached);
    assert_eq!(
        started,
        Start {
            state: State::On(None),
            recheck: false
        }
    );
    assert_eq!(told(&root, &started.state), (true, None));
}

/// What a background answer does: only a skill or a real failure changes the state.
#[test]
fn only_a_skill_or_a_failure_changes_the_state_after_a_recheck() {
    let now = State::On(None);
    let skill = skill_0_9_3();
    assert_eq!(
        start::after(&now, Found::Skill(skill.clone())),
        Some(State::On(Some(skill)))
    );
    assert_eq!(start::after(&now, Found::Broken), Some(State::Off));
    assert_eq!(start::after(&now, Found::NoSkill), None);
    assert_eq!(start::after(&now, Found::TimedOut), None);
    assert_eq!(start::first(Found::TimedOut, || None).state, now);
    assert_eq!(
        start::first(Found::Broken, || Some(skill_0_9_3())).state,
        State::Off
    );
}

/// A timeout with an older skill cached, then a background check that fails for real: the
/// launch write that follows takes the cached skill out of Claude's plugin.
#[test]
fn a_background_failure_takes_the_cached_skill_off_the_disk() {
    let root = Scratch::new("start-timeout-broken-cached");
    let (program, cached) = stetdoc_in(&root, "stetdoc 0.9.3\n");
    let old = cache_old_skill(&cached);
    slow(&root);
    let started = first(&Passed, &program, &cached);
    assert_eq!(started.state, State::On(Some(old)));
    launch_write(&root, &started.state);
    assert!(cached.join(STETDOC).join("SKILL.md").exists());

    write::remove_file(&root.join("bin").join("stub-version.txt")).unwrap();
    answers(&root);
    let next = again(&Never, &program, &cached, &started.state).unwrap();
    assert_eq!(next, State::Off);
    launch_write(&root, &next);
    assert!(!cached.join(STETDOC).exists(), "the skill's folder is gone");
    assert_eq!(cached_skill(&cached), None);
}

/// A `--version` that fails for real at the start, with an older skill cached: the launch
/// write takes the cached skill out of Claude's plugin.
#[test]
fn a_failure_at_start_takes_the_cached_skill_off_the_disk() {
    let root = Scratch::new("start-broken-cached");
    let (program, cached) = stetdoc_in(&root, "\n");
    cache_old_skill(&cached);
    let started = first(&Never, &program, &cached);
    assert_eq!(
        started,
        Start {
            state: State::Off,
            recheck: false
        }
    );
    launch_write(&root, &started.state);
    assert!(!cached.join(STETDOC).exists(), "the skill's folder is gone");
}

/// A `--version` that works, then a `skills get core` that hangs past the limit: the skill is
/// left out, stetdoc stays on, and nothing is checked again.
#[test]
fn a_skill_past_the_limit_is_left_out_with_no_recheck() {
    let root = Scratch::new("start-skill-hangs");
    let (program, cached) = stetdoc_in(&root, "stetdoc 9\n");
    write::write(&root.join("bin").join("stub-skill-hang"), b"").unwrap();
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let found = block_on(start::check(
        &launcher,
        &AtTheSkillRun::new(),
        &program,
        &cached,
        FIRST_LIMIT,
    ));
    assert_eq!(found, Found::NoSkill);
    assert_eq!(
        start::first(found, || None),
        Start {
            state: State::On(None),
            recheck: false
        }
    );
}

/// A stetdoc that cannot start (a file with no execute bit): absent at once, not checked again.
#[cfg(unix)]
#[test]
fn a_stetdoc_that_cannot_start_is_absent_and_not_checked_again() {
    let root = Scratch::new("start-not-a-program");
    let program = root.join("bin").join("stetdoc");
    write::write(&program, b"not a program\n").unwrap();
    let cached = profile_paths(&root).claude_skills();
    assert_eq!(
        first(&Never, &program, &cached),
        Start {
            state: State::Off,
            recheck: false
        }
    );
}
