//! Each OS's rule (`postcheck/judge.rs`) on plain values: what Chromium's sandboxed children
//! look like passes, and a child whose state is exactly Chrome's own (`--no-sandbox`) fails, on
//! every OS.

use super::super::judge::{self, Mode, Namespaces, Token};
use crate::browser_guard::session::{Refused, StartError};

fn token(integrity: u32, restricted: bool) -> Token {
    Token {
        integrity,
        restricted,
    }
}

/// `SECURITY_MANDATORY_LOW_RID`, `_UNTRUSTED_RID` and `_MEDIUM_RID`.
const LOW_RID: u32 = 0x1000;
const UNTRUSTED: u32 = 0;
const MEDIUM: u32 = 0x2000;

#[test]
fn windows_wants_a_lower_integrity_or_a_restriction_chrome_lacks() {
    // Chrome at Medium, unrestricted (no sandbox of the host's): Chromium's renderer (restricted,
    // Low then Untrusted) and GPU process (restricted, Low or Medium) pass.
    let chrome = Some(token(MEDIUM, false));
    for child in [
        token(LOW_RID, true),
        token(UNTRUSTED, true),
        token(MEDIUM, true),
        token(LOW_RID, false),
    ] {
        assert_eq!(judge::windows(chrome, Some(child)), Ok(()), "{child:?}");
    }
    // Chrome at Low in an AppContainer (the coming Windows design): a lockdown renderer passes.
    let low_chrome = Some(token(LOW_RID, false));
    assert_eq!(
        judge::windows(low_chrome, Some(token(UNTRUSTED, true))),
        Ok(())
    );
    assert_eq!(
        judge::windows(low_chrome, Some(token(LOW_RID, true))),
        Ok(())
    );
    // A restricted Chrome hands its restriction down: only a lower level counts then.
    let restricted_chrome = Some(token(LOW_RID, true));
    assert_eq!(
        judge::windows(restricted_chrome, Some(token(UNTRUSTED, true))),
        Ok(())
    );
    assert!(judge::windows(restricted_chrome, Some(token(LOW_RID, true))).is_err());
}

/// `--no-sandbox`: the child's token is exactly Chrome's, whatever Chrome's is.
#[test]
fn windows_refuses_a_child_whose_token_is_exactly_chromes() {
    for chrome in [
        token(MEDIUM, false),
        token(LOW_RID, false),
        token(UNTRUSTED, false),
        token(LOW_RID, true),
    ] {
        assert_eq!(
            judge::windows(Some(chrome), Some(chrome)),
            Err("runs at Chrome's own integrity level, with no restriction Chrome lacks"),
            "{chrome:?}"
        );
    }
    // A child above Chrome is no sandbox either.
    assert!(judge::windows(Some(token(LOW_RID, false)), Some(token(MEDIUM, false))).is_err());
    assert_eq!(
        judge::windows(Some(token(MEDIUM, false)), None),
        Err("has a token the guard could not read")
    );
    assert_eq!(
        judge::windows(None, Some(token(UNTRUSTED, true))),
        Err("cannot be compared with Chrome, whose token the guard could not read")
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

/// `--no-sandbox`: the child's status and namespaces are exactly Chrome's.
#[test]
fn linux_refuses_a_child_whose_state_is_exactly_chromes() {
    let chrome = ns(Some("pid:[1]"), Some("user:[1]"));
    // Chrome itself has no seccomp filter; under a host's own filter it may have one, which
    // the child then shares with Chrome's namespaces.
    for status in [
        "Name:\tchrome\nSeccomp:\t0\n",
        "Name:\tchrome\nSeccomp:\t2\n",
    ] {
        assert!(
            judge::linux(Some(status), &chrome, &chrome.clone()).is_err(),
            "{status}"
        );
    }
}

/// macOS asks `file-read-data` of Chrome's profile: 0 allowed, 1 denied, -1 not answered.
#[test]
fn macos_wants_the_child_denied_the_profile_chrome_may_read() {
    let mode = Mode::ChromeSandbox;
    assert_eq!(judge::macos(mode, 0, 1), Ok(()));
    // `--no-sandbox`: the child answers exactly as Chrome does.
    assert_eq!(
        judge::macos(mode, 0, 0),
        Err("may read Chrome's profile, as Chrome does")
    );
    // A Chrome denied its own profile cannot tell its children apart.
    for child in [0, 1, -1] {
        assert_eq!(
            judge::macos(mode, 1, child),
            Err("cannot be told apart from Chrome, which is itself denied its profile")
        );
    }
    for (chrome, child) in [(0, -1), (-1, 1), (-1, -1)] {
        assert_eq!(
            judge::macos(mode, chrome, child),
            Err("could not be asked about its Seatbelt profile")
        );
    }
}

/// macOS without `sandbox_check` refuses the browser with its own kind and sentence.
#[test]
fn a_missing_sandbox_check_is_its_own_refusal() {
    let refused = StartError::Sandbox(Refused::NoSandboxCheck);
    assert_eq!(
        refused.to_string(),
        "Chrome's sandbox cannot be checked: this macOS has no sandbox_check; the guard never \
         runs Chrome unchecked"
    );
    #[cfg(target_os = "macos")]
    assert!(matches!(
        super::super::macos::found(None),
        Err(Refused::NoSandboxCheck)
    ));
}

/// Windows, in a job that limits windows: its own refusal, before Chrome starts.
#[test]
fn a_job_that_limits_windows_is_its_own_refusal() {
    assert_eq!(
        StartError::Sandbox(Refused::JobLimitsWindows).to_string(),
        "The browser can't start its own protection here: the job it runs in limits its windows; \
         the guard never runs Chrome without it"
    );
}
