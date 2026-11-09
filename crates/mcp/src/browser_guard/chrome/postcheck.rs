//! The check that Chrome's children really run sandboxed (decision record 0029, "Chrome's
//! sandbox, never weakened", "The check after the start"). Chrome's stderr (`sandbox.rs`) tells
//! only some failures: Windows prints its broker's launch errors in debug builds only
//! (`sandbox/policy/win/sandbox_win.cc`). So once Chrome has printed its endpoint,
//! [`check_sandbox`] looks at the processes below it, at most [`CHECK_WAIT`], until it has seen a
//! renderer and every child of a [`REQUIRED`] kind sandboxed; and then, for Chrome's whole life,
//! the sentinel (`postcheck/sentinel.rs`) looks again every [`sentinel::EVERY`] at each child that
//! has appeared since. A child that is not sandboxed, or whose state cannot be read, refuses the
//! browser ([`Refused::Unsandboxed`]): Chrome is killed and never started again. No renderer at
//! the start is a failed start.
//!
//! Each child is judged against Chrome's own browser process, never against fixed marks: a child
//! that only inherits a confinement Chrome itself runs in (Chrome under `--no-sandbox` inside a
//! sandbox of the host's) is no sandboxed child. The OS parts read the processes
//! (`postcheck/linux.rs`, `postcheck/windows.rs`, `postcheck/macos.rs`) behind [`Source`]; what
//! each answer means is in `postcheck/judge.rs`, plain values in and out, tested on every OS.
//! [`Watch`] remembers each child judged sandboxed by its pid and start time, so a look reads
//! only what is new, and a reused pid is judged afresh.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(any(windows, test))]
pub(crate) mod crashed;
pub(crate) mod judge;
pub(crate) mod sentinel;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use super::super::parent::Parent;
use super::super::session::{Refused, StartError};
use super::super::{PARENT_GONE, pause};
use super::{Chrome, lock};

/// How long the check waits for Chrome's first renderer and for its children's sandbox.
pub(crate) const CHECK_WAIT: Duration = Duration::from_secs(5);
/// How often it looks again meanwhile.
pub(crate) const LOOK_EVERY: Duration = Duration::from_millis(100);

/// What a child of Chrome is, from its `--type=` switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Renderer,
    Gpu,
    Other,
}

impl Kind {
    /// Its name in the guard's log.
    fn name(self) -> &'static str {
        match self {
            Kind::Renderer => "renderer",
            Kind::Gpu => "GPU process",
            Kind::Other => "child process",
        }
    }
}

/// The kinds of child the check requires sandboxed. Windows: renderers (`sandbox_win.cc`,
/// `AddDefaultConfigForSandboxedProcess`: a `USER_LOCKDOWN` restricted token at Low integrity,
/// lowered to Untrusted) and the GPU process (`content/browser/gpu/gpu_process_host.cc`,
/// `GpuSandboxedProcessLauncherDelegate`: `USER_LIMITED`, restricted, at Low, or Medium until it
/// lowers itself where Low cannot reach the desktop). Utility processes are not required: their
/// sandbox type varies by service, and some are unsandboxed by design (`kNoSandbox`, the
/// storage service). Linux and macOS: renderers, whose sandbox does not vary. Only a renderer
/// must exist (at the start); a GPU process is judged whenever one appears, at the start or
/// later, and a GPU process that has not appeared yet is no failure.
pub(crate) const REQUIRED: &[Kind] = if cfg!(windows) {
    &[Kind::Renderer, Kind::Gpu]
} else {
    &[Kind::Renderer]
};

/// Where the check reads Chrome's children.
pub(crate) trait Source {
    /// The live processes below `chrome`, its descendants (on Windows every other process in
    /// the guard's job, which holds only the guard's own children), never a zombie; `None` when
    /// they cannot be listed.
    fn below(&mut self, chrome: u32) -> Option<Vec<u32>>;
    /// A value fixed for `pid`'s life, its start time, so a reused pid reads as another
    /// process; `None` when it cannot be read (the child is then judged at every look).
    fn stamp(&mut self, pid: u32) -> Option<u64>;
    /// `pid`'s command line, its arguments split by NULs or whitespace; `None` when unreadable.
    fn args(&mut self, pid: u32) -> Option<Vec<u8>>;
    /// Whether `pid`, below `chrome`, runs sandboxed: why not, if it does not or cannot be told.
    fn sandboxed(&mut self, chrome: u32, pid: u32) -> Result<(), &'static str>;
}

/// What one look found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    /// At least one renderer, and every child of a required kind sandboxed.
    Sandboxed,
    /// Every child of a required kind sandboxed, but no renderer yet.
    NoRenderer,
    /// A child of `kind` is not sandboxed, or cannot be told to be, for `why`.
    Unsandboxed {
        kind: &'static str,
        why: &'static str,
    },
}

/// What the looks have learnt of Chrome's children: each judged sandboxed (or of a kind not
/// required) with its start time and kind, and each failing one with its start time and how many
/// looks in a row it has failed.
#[derive(Debug, Default)]
pub(crate) struct Watch {
    passed: BTreeMap<u32, (u64, Kind)>,
    failing: BTreeMap<u32, (Option<u64>, u32)>,
}

impl Watch {
    /// One look at the processes below `chrome`. A child seen before with the same start time
    /// is not read again. A child that fails is a refusal once it has failed on more than
    /// `grace` looks in a row (a renderer the zygote has just forked has no seccomp filter yet);
    /// one that has gone meanwhile (no longer listed, or its start time changed) is skipped.
    pub(crate) fn look(
        &mut self,
        source: &mut dyn Source,
        chrome: u32,
        required: &[Kind],
        grace: u32,
    ) -> Look {
        let Some(pids) = source.below(chrome) else {
            return Look::Unsandboxed {
                kind: "child processes",
                why: "could not be listed",
            };
        };
        self.passed.retain(|pid, _| pids.contains(pid));
        self.failing.retain(|pid, _| pids.contains(pid));
        let mut renderers = 0_usize;
        for pid in pids {
            let stamp = source.stamp(pid);
            if let (Some(stamp), Some(&(seen, kind))) = (stamp, self.passed.get(&pid))
                && seen == stamp
            {
                renderers += usize::from(kind == Kind::Renderer);
                continue;
            }
            match judge_child(source, chrome, pid, required) {
                Ok(kind) => {
                    self.failing.remove(&pid);
                    if let Some(stamp) = stamp {
                        self.passed.insert(pid, (stamp, kind));
                    }
                    renderers += usize::from(kind == Kind::Renderer);
                }
                Err(_) if gone(source, chrome, pid, stamp) => {
                    self.failing.remove(&pid);
                }
                Err(refused) => {
                    let failed = self.failing.entry(pid).or_insert((stamp, 0));
                    if failed.0 != stamp {
                        *failed = (stamp, 0);
                    }
                    failed.1 += 1;
                    if failed.1 > grace {
                        return refused;
                    }
                }
            }
        }
        if renderers == 0 {
            Look::NoRenderer
        } else {
            Look::Sandboxed
        }
    }
}

/// `pid`'s kind once judged sandboxed, or not of a required kind; the failure otherwise.
fn judge_child(
    source: &mut dyn Source,
    chrome: u32,
    pid: u32,
    required: &[Kind],
) -> Result<Kind, Look> {
    let Some(args) = source.args(pid) else {
        return Err(Look::Unsandboxed {
            kind: Kind::Other.name(),
            why: "has a command line the guard could not read",
        });
    };
    let kind = kind_of(&args);
    if kind == Kind::Other || !required.contains(&kind) {
        return Ok(kind);
    }
    source
        .sandboxed(chrome, pid)
        .map(|()| kind)
        .map_err(|why| Look::Unsandboxed {
            kind: kind.name(),
            why,
        })
}

/// Whether `pid`, which just failed, has gone since it was listed with `stamp`: no longer
/// below Chrome, or another process under the same pid. A listing that fails now is no proof.
fn gone(source: &mut dyn Source, chrome: u32, pid: u32, stamp: Option<u64>) -> bool {
    match source.below(chrome) {
        Some(now) => !now.contains(&pid) || source.stamp(pid) != stamp,
        None => false,
    }
}

/// Looks up to `looks` times, `every` apart, until a look finds [`Look::Sandboxed`]. Before each
/// look `between` may end the check (Chrome exited, the guard's parent went). The last look
/// decides: a child not sandboxed refuses the browser, no renderer fails the start. `watch`
/// keeps what the looks learnt, for the sentinel after the start.
pub(crate) fn check(
    source: &mut dyn Source,
    watch: &mut Watch,
    (chrome, required): (u32, &[Kind]),
    (looks, every): (u32, Duration),
    mut between: impl FnMut() -> Result<(), StartError>,
) -> Result<(), StartError> {
    let mut last = Look::NoRenderer;
    for done in 0..looks {
        if done > 0 {
            pause(every);
        }
        between()?;
        last = watch.look(source, chrome, required, 0);
        if last == Look::Sandboxed {
            return Ok(());
        }
    }
    match last {
        Look::Sandboxed => Ok(()),
        Look::NoRenderer => Err(StartError::Other(format!(
            "Chrome started no renderer within {} s of listening, so its sandbox could not be \
             checked",
            CHECK_WAIT.as_secs()
        ))),
        Look::Unsandboxed { kind, why } => {
            Err(StartError::Sandbox(Refused::Unsandboxed { kind, why }))
        }
    }
}

/// The kind a command line names with its last `--type=` (Chrome's switches keep the last
/// value, `base/command_line.cc`). The arguments are split by NULs (Linux, macOS) and by
/// whitespace (Windows, and Linux children whose title Chrome rewrote into one string).
pub(crate) fn kind_of(args: &[u8]) -> Kind {
    let named = args
        .split(|byte| *byte == 0 || byte.is_ascii_whitespace())
        .filter_map(|arg| arg.strip_prefix(b"--type="))
        .next_back();
    match named {
        Some(b"renderer") => Kind::Renderer,
        Some(b"gpu-process") => Kind::Gpu,
        _ => Kind::Other,
    }
}

#[cfg(any(target_os = "linux", test))]
/// The descendants of `root` among `(pid, parent pid)` pairs, each once, `root` not among them.
pub(crate) fn descendants(root: u32, pairs: &[(u32, u32)]) -> Vec<u32> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(pid, parent) in pairs {
        if pid != parent {
            children.entry(parent).or_default().push(pid);
        }
    }
    let mut seen = BTreeSet::from([root]);
    let mut found = Vec::new();
    let mut next = vec![root];
    while let Some(pid) = next.pop() {
        for &child in children.get(&pid).map_or(&[][..], Vec::as_slice) {
            if seen.insert(child) {
                found.push(child);
                next.push(child);
            }
        }
    }
    found
}

/// The check, once `chrome` has printed its endpoint: what it learnt, once a renderer and every
/// child of a [`REQUIRED`] kind run sandboxed, within [`CHECK_WAIT`]. The caller drops Chrome
/// on an error, which kills it, and the failed start ends the guard: Chrome is never started
/// again. Ends early when Chrome exits or the guard's parent goes.
pub(crate) fn check_sandbox(
    chrome: &Chrome,
    parent: &Parent,
    profile: &Path,
    #[cfg(windows)] job: &super::super::job::Job,
) -> Result<Watch, StartError> {
    let pid = lock(&chrome.child).child.id();
    let mut source = source(
        profile,
        #[cfg(windows)]
        job,
    )
    .map_err(StartError::Sandbox)?;
    let looks = u32::try_from(CHECK_WAIT.as_millis() / LOOK_EVERY.as_millis())
        .unwrap_or(1)
        .saturating_add(1);
    let mut watch = Watch::default();
    check(
        &mut *source,
        &mut watch,
        (pid, REQUIRED),
        (looks, LOOK_EVERY),
        || {
            if parent.gone() {
                return Err(PARENT_GONE.to_owned().into());
            }
            if chrome.exited() {
                return Err("Chrome exited before its sandbox could be checked"
                    .to_owned()
                    .into());
            }
            Ok(())
        },
    )?;
    Ok(watch)
}

/// This OS's [`Source`] for a Chrome whose profile is `profile`, or in a `test-support` build the
/// tests' ([`SANDBOX_FOR_TESTS`]); the refusal when this OS gives no way to check.
#[cfg_attr(
    not(target_os = "macos"),
    expect(
        clippy::unnecessary_wraps,
        reason = "only macOS can lack a way to check (no sandbox_check)"
    )
)]
pub(crate) fn source<'a>(
    #[cfg_attr(
        not(target_os = "macos"),
        expect(unused_variables, reason = "only macOS asks about the profile")
    )]
    profile: &Path,
    #[cfg(windows)] job: &'a super::super::job::Job,
) -> Result<Box<dyn Source + 'a>, Refused> {
    #[cfg(target_os = "linux")]
    let real: Box<dyn Source + 'a> = Box::new(linux::Procfs::new());
    #[cfg(target_os = "macos")]
    let real: Box<dyn Source + 'a> =
        Box::new(macos::Libproc::new(profile, judge::Mode::ChromeSandbox)?);
    #[cfg(windows)]
    let real: Box<dyn Source + 'a> = Box::new(windows::InJob { job });
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let real: Box<dyn Source + 'a> = Box::new(Unknown);
    #[cfg(feature = "test-support")]
    if let Some(mode) = std::env::var_os(SANDBOX_FOR_TESTS) {
        return Ok(Box::new(ForTests {
            real,
            trusted: mode == "trusted",
        }));
    }
    Ok(real)
}

/// Windows: the watch over Chrome's children while the guard waits for its endpoint
/// (`crashed.rs`), which says whether a failed start reads as the sandbox's failure.
#[cfg(windows)]
pub(crate) struct WaitWatch<'a> {
    /// Never `None` on Windows, whose source needs no more than the job.
    source: Option<Box<dyn Source + 'a>>,
    exits: windows::Held,
    crashes: crashed::Crashes,
    chrome: u32,
}

#[cfg(windows)]
impl<'a> WaitWatch<'a> {
    /// A watch over the children of Chrome, process `chrome` in `job`.
    pub(crate) fn new(job: &'a super::super::job::Job, chrome: u32) -> WaitWatch<'a> {
        WaitWatch {
            source: source(Path::new(""), job).ok(),
            exits: windows::Held::default(),
            crashes: crashed::Crashes::default(),
            chrome,
        }
    }

    /// One look, at each poll of the wait.
    pub(crate) fn sample(&mut self) {
        if let Some(source) = self.source.as_mut() {
            self.crashes
                .sample(&mut **source, &mut self.exits, self.chrome);
        }
    }

    /// Whether a child seen sandboxed crashed during the wait (`crashed.rs`).
    pub(crate) fn sandbox_failed(&mut self) -> bool {
        self.sample();
        self.crashes.sandbox_failed(&mut self.exits)
    }
}

/// Windows: whether Chrome, whose stderr ended before its endpoint, exited with a crash's code
/// (`crashed.rs`). The exit may trail the end of stderr, so it waits for it up to
/// [`EXIT_WAIT`]; a Chrome still running then, or one that exited with an ordinary code, did not
/// crash.
#[cfg(windows)]
pub(crate) fn crashed_at_start(chrome: &Chrome) -> bool {
    for waited in 0..=EXIT_WAIT.as_millis() / LOOK_EVERY.as_millis() {
        if waited > 0 {
            pause(LOOK_EVERY);
        }
        let mut tracked = lock(&chrome.child);
        match tracked.child.try_wait() {
            Ok(Some(status)) => {
                tracked.reaped();
                return status
                    .code()
                    .is_some_and(|code| crashed::crash(code.cast_unsigned()));
            }
            Ok(None) => {}
            Err(_) => return false,
        }
    }
    false
}

/// How long [`crashed_at_start`] waits for Chrome's exit after its stderr ended.
#[cfg(windows)]
const EXIT_WAIT: Duration = Duration::from_secs(1);

/// Elsewhere nothing is known to list Chrome's children: the check refuses (fails closed).
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
struct Unknown;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
impl Source for Unknown {
    fn below(&mut self, _chrome: u32) -> Option<Vec<u32>> {
        None
    }

    fn stamp(&mut self, _pid: u32) -> Option<u64> {
        None
    }

    fn args(&mut self, _pid: u32) -> Option<Vec<u8>> {
        None
    }

    fn sandboxed(&mut self, _chrome: u32, _pid: u32) -> Result<(), &'static str> {
        Err("cannot be checked on this OS")
    }
}

/// Tests only (the `test-support` feature, never in a shipped build): in the guard's own
/// environment, `stub` makes a child sandboxed exactly when its arguments hold
/// [`STUB_SANDBOXED`] (the Chrome stand-in's children, `examples/chrome_stub.rs`), and `trusted`
/// makes every child sandboxed (CI's real Chrome, which runs there with `--no-sandbox`,
/// `tests/guard/real_chrome.rs`). The children are still listed and their kinds read as in a
/// shipped build.
#[cfg(feature = "test-support")]
pub(crate) const SANDBOX_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_SANDBOX";
/// The stand-in's argument for a child that counts as sandboxed under `stub`.
#[cfg(feature = "test-support")]
pub(crate) const STUB_SANDBOXED: &[u8] = b"--stub-sandboxed";

/// [`SANDBOX_FOR_TESTS`]'s source: this OS's listing, the tests' answer.
#[cfg(feature = "test-support")]
struct ForTests<'a> {
    real: Box<dyn Source + 'a>,
    trusted: bool,
}

#[cfg(feature = "test-support")]
impl Source for ForTests<'_> {
    fn below(&mut self, chrome: u32) -> Option<Vec<u32>> {
        self.real.below(chrome)
    }

    fn stamp(&mut self, pid: u32) -> Option<u64> {
        self.real.stamp(pid)
    }

    fn args(&mut self, pid: u32) -> Option<Vec<u8>> {
        self.real.args(pid)
    }

    fn sandboxed(&mut self, _chrome: u32, pid: u32) -> Result<(), &'static str> {
        let marked = self.real.args(pid).is_some_and(|args| {
            args.split(|byte| *byte == 0 || byte.is_ascii_whitespace())
                .any(|arg| arg == STUB_SANDBOXED)
        });
        if self.trusted || marked {
            Ok(())
        } else {
            Err("is a stand-in without its sandbox")
        }
    }
}

#[cfg(test)]
mod tests;
