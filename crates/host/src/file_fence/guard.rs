//! The fence on the guard's path, failing closed: [`check`] reads the hook input and the fence
//! file and decides, any failure or panic a refusal ([`Denial::Broken`]); [`arm`], called by the
//! guard's real entry only, makes a panic anywhere in the guard a refusal too (release builds
//! abort on a panic, which Claude Code would read as no answer) and starts a [`Watch`] that
//! refuses when the fence has not decided within [`DEADLINE`] (stdin read included), far under
//! Claude Code's 10-second hook timeout (`agent::loads::GUARD_TIMEOUT_SECONDS`). Once the fence
//! lets a call go ([`decided`]), the rest of the guard (the held-open check) runs as before,
//! failing open.

use std::ffi::OsString;
use std::io::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use serde_json::Value;

use super::decide::{Denial, FENCED_TOOLS, Fence, decide, home};
use super::real::Ground;
use super::{FLAG, load};
use crate::network_paths::resolve::Disk;
use crate::network_paths::{FOLD_CASE, NetEnv};

/// How long the fence may take, from the guard's start to its decision, before it refuses.
/// Measured on Linux at under a millisecond for a deep path (`tests::the_fence_is_fast`); two
/// seconds leaves room for a cold disk and a busy laptop, and keeps the guard's process start
/// plus the decision far inside Claude Code's 10 seconds.
pub(crate) const DEADLINE: Duration = Duration::from_secs(2);

/// What the guard's arguments say of the fence.
enum FenceArg {
    /// No [`FLAG`]: no fence (a Codex hook, or a launch that wrote no fence file).
    Absent,
    /// The flag without an absolute file and a digest that is text.
    Malformed,
    /// The fence file and its digest.
    Given(PathBuf, String),
}

/// The fence file and digest after [`FLAG`] in `args`.
fn fence_arg(args: &[OsString]) -> FenceArg {
    let Some(at) = args.iter().position(|arg| arg == FLAG) else {
        return FenceArg::Absent;
    };
    let file = args.get(at.saturating_add(1));
    let digest = args
        .get(at.saturating_add(2))
        .and_then(|digest| digest.to_str());
    match (file, digest) {
        (Some(file), Some(digest)) if Path::new(file).is_absolute() => {
            FenceArg::Given(PathBuf::from(file), digest.to_owned())
        }
        _ => FenceArg::Malformed,
    }
}

/// Whether `args` carry the fence.
pub(crate) fn wanted(args: &[OsString]) -> bool {
    !matches!(fence_arg(args), FenceArg::Absent)
}

/// The fence's refusal of the hook input `bytes`, if any, with `args` the guard's arguments and
/// `matter` the session's matter folder. `None` when `args` carry no fence, or the fence lets
/// the call go. A panic inside is caught and refuses (a build that unwinds; [`arm`] covers the
/// one that aborts).
pub(crate) fn check(args: &[OsString], bytes: &[u8], matter: Option<&Path>) -> Option<Denial> {
    let arg = fence_arg(args);
    if matches!(arg, FenceArg::Absent) {
        return None;
    }
    guarded(|| checked(arg, bytes, matter))
}

/// `decide`'s answer, or [`Denial::Broken`] when it panics.
pub(crate) fn guarded(decide: impl FnOnce() -> Option<Denial>) -> Option<Denial> {
    catch_unwind(AssertUnwindSafe(decide)).unwrap_or(Some(Denial::Broken))
}

/// [`check`] once the fence argument is read.
fn checked(arg: FenceArg, bytes: &[u8], matter: Option<&Path>) -> Option<Denial> {
    let FenceArg::Given(file, digest) = arg else {
        return Some(Denial::Broken);
    };
    let Ok(hook) = serde_json::from_slice::<Value>(bytes) else {
        return Some(Denial::Broken);
    };
    let Some(tool) = hook.get("tool_name").and_then(Value::as_str) else {
        return Some(Denial::Broken);
    };
    if !FENCED_TOOLS.contains(&tool) {
        return None;
    }
    let Some(input) = hook.get("tool_input") else {
        return Some(Denial::Broken);
    };
    let cwd = hook
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|cwd| cwd.is_absolute())
        .or_else(|| matter.map(Path::to_path_buf));
    let Some(cwd) = cwd else {
        return Some(Denial::Broken);
    };
    let Ok(file) = load(&file, &digest) else {
        return Some(Denial::Broken);
    };
    let fence = Fence::on_disk(&file, FOLD_CASE);
    let net = NetEnv::current();
    let disk = Disk::real(&net);
    let home = home();
    let ground = Ground {
        cwd: &cwd,
        home: home.as_deref(),
        matter,
        disk: &disk,
    };
    decide(&fence, tool, input, &ground)
}

/// A deadline that fires once unless disarmed first.
pub(crate) struct Watch {
    armed: Mutex<bool>,
    woken: Condvar,
}

impl Watch {
    /// Starts a thread that runs `fire` when `deadline` passes before [`Self::disarm`]; `fire`
    /// runs with the watch's lock held, so a disarm waits for it (`fire` ends the process). When
    /// no thread can start, `fire` runs now: nothing would enforce the deadline.
    pub(crate) fn arm(deadline: Duration, fire: impl FnOnce() + Send + 'static) -> Arc<Self> {
        let watch = Arc::new(Self {
            armed: Mutex::new(true),
            woken: Condvar::new(),
        });
        let held = Arc::clone(&watch);
        let fire = Arc::new(Mutex::new(Some(fire)));
        let fire_late = Arc::clone(&fire);
        let started = std::thread::Builder::new()
            .name("clauseo-fence-watch".to_owned())
            .spawn(move || {
                let armed = held.armed.lock().unwrap_or_else(PoisonError::into_inner);
                let (armed, _timed_out) = held
                    .woken
                    .wait_timeout_while(armed, deadline, |armed| *armed)
                    .unwrap_or_else(PoisonError::into_inner);
                if *armed && let Some(fire) = take(&fire_late) {
                    fire();
                }
            });
        if started.is_err()
            && let Some(fire) = take(&fire)
        {
            fire();
        }
        watch
    }

    /// Stops the watch: its `fire` never runs after this returns.
    pub(crate) fn disarm(&self) {
        *self.armed.lock().unwrap_or_else(PoisonError::into_inner) = false;
        self.woken.notify_all();
    }
}

/// The callback in `slot`, once.
fn take<F>(slot: &Mutex<Option<F>>) -> Option<F> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// The real entry's watch, when armed.
static WATCH: OnceLock<Arc<Watch>> = OnceLock::new();

/// For the guard's real entry (`pre_write_guard::run`), when `args` carry the fence: a panic
/// anywhere in the guard writes [`Denial::Broken`]'s reason and exits with the refusing code,
/// and a [`Watch`] does the same with [`Denial::Late`] at [`DEADLINE`] unless [`decided`]
/// came first. Nothing when they carry none. Never called by a test (it ends the process).
pub(crate) fn arm(args: &[OsString]) {
    if !wanted(args) {
        return;
    }
    std::panic::set_hook(Box::new(|_| refuse_now(Denial::Broken)));
    let watch = Watch::arm(DEADLINE, || refuse_now(Denial::Late));
    // Set once per process: the guard runs once.
    drop(WATCH.set(watch));
}

/// The fence has decided: the real entry's watch, if armed, stops.
pub(crate) fn decided() {
    if let Some(watch) = WATCH.get() {
        watch.disarm();
    }
}

/// Writes `denial`'s reason to stderr and ends the process with the refusing code.
#[expect(
    clippy::exit,
    reason = "the fence's deadline and panic refuse from another thread or a panic hook, where \
              no caller is left to return the code to"
)]
fn refuse_now(denial: Denial) {
    let mut errors = std::io::stderr().lock();
    // Nothing more can be done when the agent stopped reading; the exit code still decides.
    drop(
        errors
            .write_all(denial.reason().as_bytes())
            .and_then(|()| errors.write_all(b"\n"))
            .and_then(|()| errors.flush()),
    );
    std::process::exit(crate::pre_write_guard::DENY);
}
