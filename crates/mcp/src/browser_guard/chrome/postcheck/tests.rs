//! The check after Chrome's start over a fake [`Source`], and each OS's rule (`judge.rs`) on
//! plain values: a sandboxed renderer passes, an unsandboxed one refuses, an unreadable state
//! refuses, and no renderer within the bound fails the start. Also the watch during the wait for
//! Chrome's endpoint (`crashed.rs`): a failed start reads as the sandbox's failure only after a
//! child seen sandboxed crashed, and the crash codes.

use std::collections::BTreeMap;
use std::time::Duration;

use super::crashed::{Crashes, Exits, crash};
use super::judge::{self, LOW_RID, Namespaces, Token};
use super::{Kind, Look, Source, check, descendants, kind_of, look};
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
#[derive(Default)]
struct Fake {
    children: BTreeMap<u32, (Option<&'static [u8]>, Verdict)>,
    listable: bool,
    looks: u32,
    appear: u32,
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
        Some(self.children.keys().copied().collect())
    }

    fn args(&mut self, pid: u32) -> Option<Vec<u8>> {
        self.children[&pid].0.map(<[u8]>::to_vec)
    }

    fn sandboxed(&mut self, chrome: u32, pid: u32) -> Result<(), &'static str> {
        assert_eq!(chrome, CHROME);
        self.children[&pid].1
    }
}

const BOTH: &[Kind] = &[Kind::Renderer, Kind::Gpu];

/// [`check`] with `looks` looks and no pause between them.
fn run(fake: &mut Fake, required: &[Kind], looks: u32) -> Result<(), StartError> {
    check(fake, CHROME, required, (looks, Duration::ZERO), || Ok(()))
}

#[test]
fn a_sandboxed_renderer_passes_and_other_kinds_are_not_asked() {
    let mut fake = Fake::new(&[
        (1, Some(RENDERER), Ok(())),
        (2, Some(UTILITY), Err("never asked")),
        (3, Some(b"crashpad_handler"), Err("never asked")),
    ]);
    assert_eq!(look(&mut fake, CHROME, BOTH), Look::Sandboxed);
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
        // Looked again until the bound, in case the child was still lowering its token.
        assert_eq!(fake.looks, 3);
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
    let ended = check(&mut fake, CHROME, BOTH, (3, Duration::ZERO), || {
        Err(StartError::Other("Chrome exited".to_owned()))
    });
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

fn token(integrity: u32, app_container: bool, restricted: bool) -> Token {
    Token {
        integrity,
        app_container,
        restricted,
    }
}

#[test]
fn windows_wants_low_integrity_and_a_restricted_or_app_container_token() {
    const UNTRUSTED: u32 = 0;
    const MEDIUM: u32 = 0x2000;
    // Chromium's renderer (Low, then Untrusted) and GPU process (Low), restricted; an
    // AppContainer renderer.
    assert_eq!(judge::windows(Some(token(LOW_RID, false, true))), Ok(()));
    assert_eq!(judge::windows(Some(token(UNTRUSTED, false, true))), Ok(()));
    assert_eq!(judge::windows(Some(token(LOW_RID, true, false))), Ok(()));
    // `--no-sandbox`: the user's own token, Medium, unrestricted.
    assert_eq!(
        judge::windows(Some(token(MEDIUM, false, false))),
        Err("runs above Low integrity")
    );
    // Restricted but Medium (a GPU process not lowered yet).
    assert_eq!(
        judge::windows(Some(token(MEDIUM, false, true))),
        Err("runs above Low integrity")
    );
    assert_eq!(
        judge::windows(Some(token(LOW_RID, false, false))),
        Err("has neither a restricted nor an AppContainer token")
    );
    assert_eq!(
        judge::windows(None),
        Err("has a token the guard could not read")
    );
}

fn ns(pid: Option<&str>, user: Option<&str>) -> Namespaces {
    Namespaces {
        pid: pid.map(str::to_owned),
        user: user.map(str::to_owned),
    }
}

#[test]
fn linux_wants_a_seccomp_filter_and_another_pid_or_user_namespace() {
    let chrome = ns(Some("pid:[1]"), Some("user:[1]"));
    let sandboxed = ns(Some("pid:[2]"), Some("user:[2]"));
    let filtered = "Name:\tchrome\nNoNewPrivs:\t1\nSeccomp:\t2\nSeccomp_filters:\t1\n";
    assert_eq!(judge::linux(Some(filtered), &chrome, &sandboxed), Ok(()));
    // The setuid sandbox: a new PID namespace, the same user namespace.
    let setuid = ns(Some("pid:[2]"), Some("user:[1]"));
    assert_eq!(judge::linux(Some(filtered), &chrome, &setuid), Ok(()));
    for status in [
        "Name:\tchrome\nSeccomp:\t0\n",
        "Seccomp:\t1\n",
        "Name:\tchrome\n",
    ] {
        assert_eq!(
            judge::linux(Some(status), &chrome, &sandboxed),
            Err("has no seccomp filter"),
            "{status}"
        );
    }
    let shares = Err("shares Chrome's PID and user namespaces, or they could not be read");
    assert_eq!(judge::linux(Some(filtered), &chrome, &chrome), shares);
    assert_eq!(
        judge::linux(Some(filtered), &chrome, &ns(None, None)),
        shares
    );
    assert_eq!(
        judge::linux(Some(filtered), &ns(None, None), &sandboxed),
        shares
    );
    assert_eq!(
        judge::linux(None, &chrome, &sandboxed),
        Err("has a status the guard could not read")
    );
}

#[test]
fn macos_wants_sandbox_check_to_answer_one() {
    assert_eq!(judge::macos(1), Ok(()));
    assert_eq!(judge::macos(0), Err("runs without a Seatbelt profile"));
    assert_eq!(
        judge::macos(-1),
        Err("could not be asked about its Seatbelt profile")
    );
}

/// The real `/proc` reader on this machine: the test's own child, a `sleep`, is below it with
/// its arguments, and it shares the test's namespaces, so it is not sandboxed.
#[cfg(target_os = "linux")]
#[test]
fn linux_procfs_lists_a_child_and_judges_it_unsandboxed() {
    #[expect(
        clippy::disallowed_methods,
        reason = "a plain child for the /proc reader to find"
    )]
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let own = std::process::id();
    let mut procfs = super::linux::Procfs;
    let below = procfs.below(own).unwrap();
    assert!(below.contains(&child.id()), "{below:?}");
    let args = procfs.args(child.id()).unwrap();
    assert_eq!(args, b"sleep\x0030\x00");
    assert!(procfs.sandboxed(own, child.id()).is_err());
    assert_eq!(super::linux::parent_in("12 (a) b) c) S 34 12"), Some(34));
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
    for code in [0xC000_0005, 0xC000_0022, 0x8000_0003, 0xC000_0409] {
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
fn a_crash_code_is_an_error_ntstatus_or_a_breakpoint() {
    for code in [
        0xC000_0005,
        0xC000_0022,
        0xC000_0409,
        0xFFFF_FFFF,
        0x8000_0003,
    ] {
        assert!(crash(code), "{code:#x}");
    }
    for code in [0, 1, 3, 259, 0x7FFF_FFFF, 0x8000_0005, 0x4000_0015] {
        assert!(!crash(code), "{code:#x}");
    }
}
