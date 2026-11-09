//! What each OS's answers about a child of Chrome mean (`postcheck.rs`): plain values in, a
//! verdict out, so every OS's rule is tested on every OS. Each refusal names a fixed reason,
//! read after the child's kind in the guard's log ("its renderer runs above Low integrity").

/// Windows: what a child's process token says (`postcheck/windows.rs`).
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    /// The last sub-authority of `TokenIntegrityLevel`'s SID: its mandatory level's RID.
    pub(crate) integrity: u32,
    /// `TokenIsAppContainer`.
    pub(crate) app_container: bool,
    /// `IsTokenRestricted`: the token has restricting SIDs.
    pub(crate) restricted: bool,
}

/// `SECURITY_MANDATORY_LOW_RID`; Untrusted (0) is below it.
#[cfg(any(windows, test))]
pub(crate) const LOW_RID: u32 = 0x1000;

/// Windows: sandboxed when the token is at Low integrity or below, and restricted or an
/// `AppContainer`'s. Chromium's renderer token is `USER_LOCKDOWN`, restricted, at Low, lowered to
/// Untrusted (`sandbox/policy/win/sandbox_win.cc`, `AddDefaultConfigForSandboxedProcess`); its
/// GPU process token `USER_LIMITED`, restricted, at Low (`gpu_process_host.cc`); both restricted
/// levels add restricting SIDs (`sandbox/win/src/restricted_token_utils.cc`). An `AppContainer`
/// token (the renderer's under `RendererAppContainer`, off by default) runs at Low and has no
/// restricting SIDs. A token that cannot be read (`None`) is not sandboxed.
#[cfg(any(windows, test))]
pub(crate) fn windows(token: Option<Token>) -> Result<(), &'static str> {
    let Some(token) = token else {
        return Err("has a token the guard could not read");
    };
    if token.integrity > LOW_RID {
        return Err("runs above Low integrity");
    }
    if !token.restricted && !token.app_container {
        return Err("has neither a restricted nor an AppContainer token");
    }
    Ok(())
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

/// macOS: sandboxed when `sandbox_check(pid, NULL, SANDBOX_FILTER_NONE)` answers 1 (it answers 0
/// for a process with no Seatbelt profile and -1 on an error, which is not sandboxed either).
#[cfg(any(target_os = "macos", test))]
pub(crate) fn macos(answer: i32) -> Result<(), &'static str> {
    match answer {
        1 => Ok(()),
        0 => Err("runs without a Seatbelt profile"),
        _ => Err("could not be asked about its Seatbelt profile"),
    }
}
