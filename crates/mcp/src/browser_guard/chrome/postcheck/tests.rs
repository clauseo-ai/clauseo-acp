//! The check after Chrome's start over a fake [`Source`], and each OS's rule (`judge.rs`) on
//! plain values: a sandboxed renderer passes, an unsandboxed one refuses, a child whose state is
//! exactly Chrome's own (`--no-sandbox`) refuses on every OS, an unreadable state refuses, and no
//! renderer within the bound fails the start. The sentinel (`sentinel.rs`) catches a child that
//! turns up unsandboxed later. Also the watch during the wait for Chrome's endpoint
//! (`crashed.rs`): a failed start reads as the sandbox's failure only after a child seen
//! sandboxed crashed, and the crash codes.

use std::collections::BTreeMap;
use std::time::Duration;

use super::crashed::{Crashes, Exits, crash};
use super::sentinel::keep_looking;
use super::{Kind, Look, Source, Watch, check, descendants, kind_of};
use crate::browser_guard::session::{Refused, StartError};

const CHROME: u32 = 100;
const RENDERER: &[u8] = b"/opt/chrome\0--type=renderer\0--lang=en\0";
const GPU: &[u8] = b"/opt/chrome\0--type=gpu-process\0";
const UTILITY: &[u8] = b"/opt/chrome\0--type=utility\0--utility-sub-type=storage.mojom\0";

/// A child as a test gives it: its pid, its arguments (`None`: unreadable), its verdict.
type Given = (u32, Option<&'static [u8]>, Verdict);
/// A sandbox verdict.
type Verdict = Result<(), &'static str>;

/// Chrome's children as a test sets them: each pid's arguments (`None`: unreadable) and its
/// sandbox verdict. `looks` counts the listings; `appear` hides every child until that many.
/// A child's start time is `born`'s entry, else its pid; `asked` counts the sandbox questions,
/// and `vanish` drops that child from the listing once it has been asked about.
#[derive(Default)]
struct Fake {
    children: BTreeMap<u32, (Option<&'static [u8]>, Verdict)>,
    listable: bool,
    looks: u32,
    appear: u32,
    born: BTreeMap<u32, u64>,
    asked: u32,
    vanish: Option<u32>,
    /// A child that has gone once its command line was asked for.
    unlisted_after_args: Option<u32>,
    /// A child, unsandboxed, that is listed from look `.0` on: its pid and arguments.
    late: Option<(u32, u32, &'static [u8])>,
}

impl Fake {
    fn new(children: &[Given]) -> Fake {
        Fake {
            children: children
                .iter()
                .map(|&(pid, args, verdict)| (pid, (args, verdict)))
                .collect(),
            listable: true,
            ..Fake::default()
        }
    }
}

impl Source for Fake {
    fn below(&mut self, chrome: u32) -> Option<Vec<u32>> {
        assert_eq!(chrome, CHROME);
        self.looks += 1;
        if !self.listable {
            return None;
        }
        if self.looks <= self.appear {
            return Some(Vec::new());
        }
        if let Some((from, pid, args)) = self.late
            && self.looks >= from
        {
            let why = "runs at Chrome's own integrity level, with no restriction Chrome lacks";
            self.children.entry(pid).or_insert((Some(args), Err(why)));
        }
        Some(self.children.keys().copied().collect())
    }

    fn stamp(&mut self, pid: u32) -> Option<u64> {
        self.children
            .contains_key(&pid)
            .then(|| self.born.get(&pid).copied().unwrap_or(u64::from(pid)))
    }

    fn args(&mut self, pid: u32) -> Option<Vec<u8>> {
        let args = self.children[&pid].0.map(<[u8]>::to_vec);
        if self.unlisted_after_args == Some(pid) {
            self.children.remove(&pid);
        }
        args
    }

    fn sandboxed(&mut self, chrome: u32, pid: u32) -> Result<(), &'static str> {
        assert_eq!(chrome, CHROME);
        self.asked += 1;
        let verdict = self.children[&pid].1;
        if self.vanish == Some(pid) {
            self.children.remove(&pid);
        }
        verdict
    }
}

const BOTH: &[Kind] = &[Kind::Renderer, Kind::Gpu];

/// [`check`] with `looks` looks and no pause between them.
fn run(fake: &mut Fake, required: &[Kind], looks: u32) -> Result<(), StartError> {
    let mut watch = Watch::default();
    check(
        fake,
        &mut watch,
        (CHROME, required),
        (looks, Duration::ZERO),
        || Ok(()),
    )
}

/// One look with no grace and nothing learnt before.
fn look(fake: &mut Fake, required: &[Kind]) -> Look {
    Watch::default().look(fake, CHROME, required, 0)
}

#[test]
fn a_sandboxed_renderer_passes_and_other_kinds_are_not_asked() {
    let mut fake = Fake::new(&[
        (1, Some(RENDERER), Ok(())),
        (2, Some(UTILITY), Err("never asked")),
        (3, Some(b"crashpad_handler"), Err("never asked")),
    ]);
    assert_eq!(look(&mut fake, BOTH), Look::Sandboxed);
    assert_eq!(run(&mut fake, BOTH, 3), Ok(()));
}

#[test]
fn an_unsandboxed_renderer_or_gpu_process_refuses_the_browser() {
    for (args, kind) in [(RENDERER, "renderer"), (GPU, "GPU process")] {
        let mut fake = Fake::new(&[
            (1, Some(RENDERER), Ok(())),
            (2, Some(args), Err("runs above Low integrity")),
        ]);
        assert_eq!(
            run(&mut fake, BOTH, 3),
            Err(StartError::Sandbox(Refused::Unsandboxed {
                kind,
                why: "runs above Low integrity"
            }))
        );
        // Looked again until the bound, in case the child was still lowering its token: three
        // looks, each failure listing the children once more (to tell a child that has gone).
        assert_eq!(fake.looks, 6);
    }
    // Linux and macOS require renderers only: an unsandboxed GPU process is not asked.
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(())), (2, Some(GPU), Err("x"))]);
    assert_eq!(run(&mut fake, &[Kind::Renderer], 1), Ok(()));
}

#[test]
fn an_unreadable_state_refuses_the_browser() {
    let unreadable = Err("has a token the guard could not read");
    let mut fake = Fake::new(&[(1, Some(RENDERER), unreadable)]);
    assert_eq!(
        run(&mut fake, BOTH, 2),
        Err(StartError::Sandbox(Refused::Unsandboxed {
            kind: "renderer",
            why: "has a token the guard could not read"
        }))
    );
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(())), (2, None, Ok(()))]);
    assert!(matches!(
        run(&mut fake, BOTH, 2),
        Err(StartError::Sandbox(Refused::Unsandboxed {
            kind: "child process",
            ..
        }))
    ));
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(()))]);
    fake.listable = false;
    assert!(matches!(
        run(&mut fake, BOTH, 2),
        Err(StartError::Sandbox(Refused::Unsandboxed {
            kind: "child processes",
            ..
        }))
    ));
}

#[test]
fn no_renderer_within_the_bound_fails_the_start_and_is_no_pass() {
    for children in [
        &[][..],
        &[(2, Some(GPU), Ok(())), (3, Some(UTILITY), Ok(()))][..],
    ] {
        let mut fake = Fake::new(children);
        let failed = run(&mut fake, BOTH, 4);
        assert!(
            matches!(&failed, Err(StartError::Other(reason)) if reason.contains("no renderer")),
            "{failed:?}"
        );
        assert_eq!(fake.looks, 4);
    }
}

#[test]
fn a_renderer_that_appears_within_the_bound_passes() {
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(()))]);
    fake.appear = 2;
    assert_eq!(run(&mut fake, BOTH, 3), Ok(()));
    assert_eq!(fake.looks, 3);
}

#[test]
fn the_check_ends_when_chrome_exits_or_the_parent_goes() {
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(()))]);
    let ended = check(
        &mut fake,
        &mut Watch::default(),
        (CHROME, BOTH),
        (3, Duration::ZERO),
        || Err(StartError::Other("Chrome exited".to_owned())),
    );
    assert_eq!(ended, Err(StartError::Other("Chrome exited".to_owned())));
    assert_eq!(fake.looks, 0);
}

#[test]
fn the_kind_is_the_last_type_switch_split_by_nuls_or_whitespace() {
    assert_eq!(kind_of(RENDERER), Kind::Renderer);
    assert_eq!(kind_of(GPU), Kind::Gpu);
    assert_eq!(kind_of(UTILITY), Kind::Other);
    assert_eq!(
        kind_of(b"\"C:\\Chrome\\chrome.exe\" --type=renderer --x"),
        Kind::Renderer
    );
    // Chrome's rewritten process title on Linux: one string, NUL padded.
    assert_eq!(
        kind_of(b"/opt/chrome --type=gpu-process --y\0\0\0"),
        Kind::Gpu
    );
    assert_eq!(
        kind_of(b"chrome --type=renderer --type=utility"),
        Kind::Other
    );
    assert_eq!(kind_of(b"chrome --type=rendererx"), Kind::Other);
    assert_eq!(kind_of(b"chrome x--type=renderer"), Kind::Other);
    assert_eq!(kind_of(b""), Kind::Other);
}

#[test]
fn descendants_are_the_whole_tree_below_chrome_and_nothing_else() {
    let pairs = [
        (5, 1),
        (CHROME, 5),
        (7, CHROME),
        (8, 7),
        (9, 8),
        (10, 1),
        (11, 11),
    ];
    let mut found = descendants(CHROME, &pairs);
    found.sort_unstable();
    assert_eq!(found, vec![7, 8, 9]);
    // A cycle (a reused pid naming a descendant as its parent) ends.
    let mut found = descendants(CHROME, &[(7, CHROME), (CHROME, 7)]);
    found.sort_unstable();
    assert_eq!(found, vec![7]);
}

/// The real `/proc` reader on this machine: the test's own child, a `sleep`, is below it with
/// its arguments, and it shares the test's namespaces, so it is not sandboxed.
#[cfg(target_os = "linux")]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "a plain child for the /proc reader to find, waited for until it has run exec"
)]
fn linux_procfs_lists_a_child_and_judges_it_unsandboxed() {
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let own = std::process::id();
    let mut procfs = super::linux::Procfs::new();
    // Until the child has run `exec`, its command line is still the test's own: wait for it, at
    // most 10 s.
    let mut args = Vec::new();
    for _ in 0..1_000 {
        args = procfs.args(child.id()).unwrap_or_default();
        if args == b"sleep\x0030\x00" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(args, b"sleep\x0030\x00");
    let below = procfs.below(own).unwrap();
    assert!(below.contains(&child.id()), "{below:?}");
    assert!(procfs.stamp(child.id()).is_some());
    assert!(procfs.sandboxed(own, child.id()).is_err());
    assert_eq!(
        super::linux::parse_stat(
            "12 (a) b) c) S 34 12 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 99 x"
        ),
        Some(super::linux::Stat {
            parent: 34,
            started: 99,
            zombie: false
        })
    );
    assert!(
        super::linux::parse_stat("12 (a) Z 34 12 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 99")
            .unwrap()
            .zombie
    );
    child.kill().unwrap();
    child.wait().unwrap();
}

/// Exit codes as a test sets them; which pids were held.
#[derive(Default)]
struct FakeExits {
    codes: BTreeMap<u32, u32>,
    held: Vec<u32>,
}

impl Exits for FakeExits {
    fn hold(&mut self, pid: u32) {
        self.held.push(pid);
    }

    fn exit_code(&mut self, pid: u32) -> Option<u32> {
        assert!(self.held.contains(&pid), "{pid} read before it was held");
        self.codes.get(&pid).copied()
    }
}

/// The watch over `children`, sampled twice while they run, then the children gone with their
/// exits set to `codes`: whether the failed start is the sandbox's.
fn failed_start(children: &[Given], codes: &[(u32, u32)]) -> bool {
    let mut fake = Fake::new(children);
    let mut exits = FakeExits::default();
    let mut crashes = Crashes::default();
    crashes.sample(&mut fake, &mut exits, CHROME);
    crashes.sample(&mut fake, &mut exits, CHROME);
    let mut held = exits.held.clone();
    held.dedup();
    assert_eq!(held, exits.held, "each child held once");
    // The children have gone from the listing; only the held handles still tell their exits.
    fake.children.clear();
    exits.codes = codes.iter().copied().collect();
    crashes.sample(&mut fake, &mut exits, CHROME);
    crashes.sandbox_failed(&mut exits)
}

#[test]
fn a_sandboxed_child_that_crashed_during_the_start_is_the_sandboxs_refusal() {
    let sandboxed = [(1, Some(RENDERER), Ok(()))];
    for code in [0xC000_0005, 0x8000_0003, 7006, 7013] {
        assert!(failed_start(&sandboxed, &[(1, code)]), "{code:#x}");
    }
    // Any child kind: only its token counts.
    assert!(failed_start(
        &[(1, Some(b"chrome"), Ok(()))],
        &[(1, 0xC000_0005)]
    ));
}

#[test]
fn any_other_failed_start_stays_the_ordinary_one() {
    // No child at all (a slow first start); a sandboxed child still running or ended plainly.
    assert!(!failed_start(&[], &[]));
    let sandboxed = [(1, Some(RENDERER), Ok(()))];
    assert!(!failed_start(&sandboxed, &[]));
    assert!(!failed_start(&sandboxed, &[(1, 0)]));
    assert!(!failed_start(&sandboxed, &[(1, 1)]));
    assert!(!failed_start(&sandboxed, &[(1, 0x8000_0005)]));
    // A crash of a child never seen sandboxed.
    let unsandboxed = [(1, Some(RENDERER), Err("runs above Low integrity"))];
    assert!(!failed_start(&unsandboxed, &[(1, 0xC000_0005)]));
}

#[test]
fn a_crash_code_is_a_breakpoint_an_access_violation_or_the_sandboxs_own() {
    for code in [
        0x8000_0003,
        0xC000_0005,
        7006,
        7007,
        7008,
        7009,
        7010,
        7011,
        7013,
    ] {
        assert!(crash(code), "{code:#x}");
    }
    // The loader's codes (a DLL not found, a DLL's start failed, a bad image), other errors,
    // the job's memory limit and the broker's shutdown, and Chrome's own small codes.
    for code in [
        0xC000_0135,
        0xC000_0142,
        0xC000_007B,
        0xC000_0022,
        0xC000_0409,
        0xFFFF_FFFF,
        7012,
        7014,
        0,
        1,
        3,
        259,
        0x7FFF_FFFF,
        0x8000_0005,
        0x4000_0015,
    ] {
        assert!(!crash(code), "{code:#x}");
    }
}

/// A failed start whose sandboxed child died of a loader error is the ordinary one.
#[test]
fn a_child_that_died_of_a_loader_error_stays_the_ordinary_failed_start() {
    let sandboxed = [(1, Some(RENDERER), Ok(()))];
    for code in [0xC000_0135, 0xC000_0142, 0xC000_007B] {
        assert!(!failed_start(&sandboxed, &[(1, code)]), "{code:#x}");
    }
}

#[test]
fn a_child_judged_once_is_not_read_again_until_its_pid_is_reused() {
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(())), (2, Some(GPU), Ok(()))]);
    let mut watch = Watch::default();
    for _ in 0..3 {
        assert_eq!(watch.look(&mut fake, CHROME, BOTH, 0), Look::Sandboxed);
    }
    assert_eq!(fake.asked, 2, "each child asked about once");
    // Pid 2 now names another process, unsandboxed: judged afresh, and refused.
    fake.born.insert(2, 7);
    fake.children.insert(2, (Some(GPU), Err("x")));
    assert_eq!(
        watch.look(&mut fake, CHROME, BOTH, 0),
        Look::Unsandboxed {
            kind: "GPU process",
            why: "x"
        }
    );
}

#[test]
fn a_child_that_vanishes_mid_check_is_skipped_not_refused() {
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(())), (2, Some(RENDERER), Err("x"))]);
    fake.vanish = Some(2);
    assert_eq!(look(&mut fake, BOTH), Look::Sandboxed);
    // One whose command line can no longer be read because it has gone.
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(())), (2, None, Ok(()))]);
    fake.unlisted_after_args = Some(2);
    assert_eq!(look(&mut fake, BOTH), Look::Sandboxed);
    // A child that is still listed after it failed is refused.
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(())), (2, Some(RENDERER), Err("x"))]);
    assert!(matches!(look(&mut fake, BOTH), Look::Unsandboxed { .. }));
}

#[test]
fn a_failing_child_is_refused_only_after_the_grace() {
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(())), (2, Some(RENDERER), Err("x"))]);
    let mut watch = Watch::default();
    for _ in 0..3 {
        assert_eq!(watch.look(&mut fake, CHROME, BOTH, 3), Look::Sandboxed);
    }
    assert!(matches!(
        watch.look(&mut fake, CHROME, BOTH, 3),
        Look::Unsandboxed { .. }
    ));
    // A child that passes within the grace (the seccomp filter came) is never refused.
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(())), (2, Some(RENDERER), Err("x"))]);
    let mut watch = Watch::default();
    assert_eq!(watch.look(&mut fake, CHROME, BOTH, 3), Look::Sandboxed);
    fake.children.insert(2, (Some(RENDERER), Ok(())));
    for _ in 0..5 {
        assert_eq!(watch.look(&mut fake, CHROME, BOTH, 3), Look::Sandboxed);
    }
}

/// The sentinel: a child that turns up unsandboxed after the start (a renderer, or a GPU
/// process, which Windows requires) is caught, after the grace; while Chrome runs with only
/// sandboxed children it goes on, and Chrome's end stops it quietly.
#[test]
fn the_sentinel_catches_a_child_that_turns_up_unsandboxed_after_the_start() {
    for (args, kind) in [(RENDERER, "renderer"), (GPU, "GPU process")] {
        let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(()))]);
        let mut watch = Watch::default();
        assert_eq!(run_on(&mut fake, &mut watch), Ok(()));
        // It appears at the sentinel's fourth look, with Chrome's own state (each failure also
        // lists the children again, to tell a child that has gone).
        fake.late = Some((fake.looks + 4, 9, args));
        let mut waits = 0;
        let refused = keep_looking(&mut fake, watch, (CHROME, BOTH, 2), || {
            waits += 1;
            waits < 50
        });
        assert_eq!(
            refused,
            Some(Refused::Unsandboxed {
                kind,
                why: "runs at Chrome's own integrity level, with no restriction Chrome lacks"
            })
        );
        // Seen at the fourth look, failing there and at the fifth (a grace of two looks), and
        // refused at its third failure: six looks.
        assert_eq!(waits, 6);
    }
    let mut fake = Fake::new(&[(1, Some(RENDERER), Ok(()))]);
    let mut waits = 0;
    let ended = keep_looking(&mut fake, Watch::default(), (CHROME, BOTH, 2), || {
        waits += 1;
        waits <= 20
    });
    assert_eq!(ended, None);
    assert_eq!(fake.looks, 20);
}

fn run_on(fake: &mut Fake, watch: &mut Watch) -> Result<(), StartError> {
    check(fake, watch, (CHROME, BOTH), (3, Duration::ZERO), || Ok(()))
}

mod judge;
