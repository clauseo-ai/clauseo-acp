//! What each OS's answers about a child of Chrome mean (`postcheck.rs`): plain values in, a
//! verdict out, so every OS's rule is tested on every OS. Each refusal names a fixed reason,
//! read after the child's kind in the guard's log ("its renderer runs at Chrome's own
//! integrity, unrestricted"). Each rule compares the child with Chrome's own browser process, so a
//! child that merely inherits the browser's confinement (Chrome under `--no-sandbox` inside a
//! sandbox of the host's own) never passes.

/// Windows: what a process token says (`postcheck/windows.rs`).
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    /// The last sub-authority of `TokenIntegrityLevel`'s SID: its mandatory level's RID.
    pub(crate) integrity: u32,
    /// `IsTokenRestricted`: the token has restricting SIDs (`TokenRestrictedSids` is not
    /// empty; the call is documented as exactly that test).
    pub(crate) restricted: bool,
}

/// Windows: a child is sandboxed when its token is restricted and Chrome's own is not, or when
/// it runs at an integrity level strictly below Chrome's. Chromium's renderer runs on a
/// `USER_LOCKDOWN` primary token, whose restricting SID list holds the null SID, at Low lowered
/// to Untrusted (`sandbox/policy/win/sandbox_win.cc`, `AddDefaultConfigForSandboxedProcess`;
/// `sandbox/win/src/restricted_token_utils.cc`); its GPU process on `USER_LIMITED`, restricted
/// too (`content/browser/gpu/gpu_process_host.cc`). A child under `--no-sandbox` inherits
/// Chrome's token as it is: the same integrity, restricted only if Chrome's is, and fails both
/// tests even when Chrome itself runs in a sandbox of the host's (Low, an `AppContainer`).
/// Restriction counts only when Chrome's token has none, since a restricted Chrome hands its
/// restriction down unchanged. A token that cannot be read, Chrome's or the child's, refuses.
#[cfg(any(windows, test))]
pub(crate) fn windows(chrome: Option<Token>, child: Option<Token>) -> Result<(), &'static str> {
    let Some(chrome) = chrome else {
        return Err("cannot be compared with Chrome, whose token the guard could not read");
    };
    let Some(child) = child else {
        return Err("has a token the guard could not read");
    };
    if child.integrity < chrome.integrity || (child.restricted && !chrome.restricted) {
        return Ok(());
    }
    Err("runs at Chrome's own integrity level, with no restriction Chrome lacks")
}

/// Linux: a child's namespaces, each `readlink /proc/<pid>/ns/<name>` (`pid:[4026531836]`), or
/// `None` when unreadable.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Namespaces {
    pub(crate) pid: Option<String>,
    pub(crate) user: Option<String>,
}

/// Linux: sandboxed when `/proc/<pid>/status` says `Seccomp: 2` (a seccomp-BPF filter, which
/// Chromium's renderer sandbox installs, `sandbox/policy/linux/sandbox_seccomp_bpf_linux.cc`)
/// and the child is in another PID or user namespace than Chrome (the namespace sandbox,
/// `sandbox/linux/services/namespace_sandbox.cc`, or the setuid sandbox's new PID namespace,
/// `sandbox/linux/suid/sandbox.c`). `status` is `None` when unreadable; a namespace that cannot
/// be read on either side does not count as another.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn linux(
    status: Option<&str>,
    chrome: &Namespaces,
    child: &Namespaces,
) -> Result<(), &'static str> {
    let Some(status) = status else {
        return Err("has a status the guard could not read");
    };
    let seccomp = status
        .lines()
        .find_map(|line| line.strip_prefix("Seccomp:"))
        .map(str::trim);
    if seccomp != Some("2") {
        return Err("has no seccomp filter");
    }
    let other = |ours: &Option<String>, theirs: &Option<String>| match (ours, theirs) {
        (Some(ours), Some(theirs)) => ours != theirs,
        _ => false,
    };
    if !other(&chrome.pid, &child.pid) && !other(&chrome.user, &child.user) {
        return Err("shares Chrome's PID and user namespaces, or they could not be read");
    }
    Ok(())
}

/// macOS: what the check expects of Chrome's children (`postcheck/macos.rs`). One mode now;
/// the browser profile of the coming macOS design adds a second, `BrowserProfile` (Chrome runs
/// `--no-sandbox` inside the host's own Seatbelt profile, and a child passes when it is in that
/// profile), which a later change switches on once the guard has proved it runs there. Until
/// then nothing builds it, and each match on [`Mode`] must handle it when it comes.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Chrome's own sandbox: each child under its own Seatbelt profile, Chrome under none that
    /// keeps it from its profile folder.
    ChromeSandbox,
}

/// macOS: `sandbox_check(pid, "file-read-data", SANDBOX_FILTER_PATH, <profile>/Default/
/// Preferences)` for Chrome and for the child: 0 allowed, 1 denied, -1 not answered. In
/// [`Mode::ChromeSandbox`] the child must be denied what Chrome is allowed. Chromium's renderer
/// profile starts from `(deny default)` (`sandbox/policy/mac/common.sb`) and grants reads of
/// the bundle, system paths and a few named preference files, never the user data directory,
/// whose parameter it defines and leaves unused (`common.sb`, `renderer.sb`); Chrome's browser
/// process must read its own profile to run. A child that inherits Chrome's state answers as
/// Chrome does and fails. A Chrome that is denied its own profile, or either answer missing,
/// cannot tell them apart, and refuses.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn macos(mode: Mode, chrome: i32, child: i32) -> Result<(), &'static str> {
    match mode {
        Mode::ChromeSandbox => match (chrome, child) {
            (0, 1) => Ok(()),
            (0, 0) => Err("may read Chrome's profile, as Chrome does"),
            (1, _) => Err("cannot be told apart from Chrome, which is itself denied its profile"),
            _ => Err("could not be asked about its Seatbelt profile"),
        },
    }
}
