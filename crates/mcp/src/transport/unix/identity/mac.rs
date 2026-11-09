//! macOS only: the pid and pidversion in an audit token, through libbsm's accessors (declared
//! here: neither `libc` nor `nix` has them), and this process's own token through
//! `task_info(TASK_AUDIT_TOKEN)`. The pidversion changes when a pid is reused and at each exec.
//! Also this process's thread count, through `proc_pidinfo(PROC_PIDTASKINFO)`.
#![expect(
    unsafe_code,
    reason = "task_info, proc_pidinfo and libbsm's audit_token_to_pid and \
              audit_token_to_pidversion have no safe wrapper in libc or nix"
)]

use nix::sys::socket::audit_token_t;

use super::super::super::Identity;

/// `TASK_AUDIT_TOKEN`, `task_info`'s flavor for the task's audit token (`mach/task_info.h`).
const TASK_AUDIT_TOKEN: libc::task_flavor_t = 15;
/// `TASK_AUDIT_TOKEN_COUNT`: the token's size in `natural_t`s, eight.
const TASK_AUDIT_TOKEN_COUNT: libc::mach_msg_type_number_t = 8;

unsafe extern "C" {
    /// The port naming this task, which `mach_task_self()` reads (`mach/mach_init.h`); libc's
    /// own wrapper is deprecated.
    static mach_task_self_: libc::mach_port_t;
}

#[link(name = "bsm")]
unsafe extern "C" {
    /// `bsm/libbsm.h`: the pid in `token`.
    safe fn audit_token_to_pid(token: audit_token_t) -> libc::pid_t;
    /// `bsm/libbsm.h`: the pidversion in `token`.
    safe fn audit_token_to_pidversion(token: audit_token_t) -> libc::c_int;
}

/// This process's audit token, `None` when the kernel refuses.
pub(super) fn own_token() -> Option<audit_token_t> {
    let mut token = audit_token_t::default();
    let mut count = TASK_AUDIT_TOKEN_COUNT;
    // SAFETY: `mach_task_self_` is set before `main` and never changes; `task_info` writes at most
    // `count` (eight) `natural_t`s into `token`, which is eight `c_uint`s, and the new count into
    // `count`; both live until the call returns.
    let result = unsafe {
        libc::task_info(
            mach_task_self_,
            TASK_AUDIT_TOKEN,
            (&raw mut token).cast(),
            &raw mut count,
        )
    };
    (result == libc::KERN_SUCCESS && count == TASK_AUDIT_TOKEN_COUNT).then_some(token)
}

/// The identity in `token`, `None` for a pid or version below 0.
pub(super) fn identity(token: audit_token_t) -> Option<Identity> {
    let pid = u32::try_from(audit_token_to_pid(token)).ok()?;
    let version = u64::try_from(audit_token_to_pidversion(token)).ok()?;
    Some(Identity { pid, version })
}

/// This process's thread count, from `proc_pidinfo(PROC_PIDTASKINFO)`; `None` when it fails.
pub(super) fn threads() -> Option<usize> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_taskinfo>()).ok()?;
    let pid = libc::pid_t::try_from(std::process::id()).ok()?;
    // SAFETY: `info` is writable for `size` bytes, a whole `proc_taskinfo`, and lives until the
    // call returns; `proc_pidinfo` writes at most `size` bytes and returns how many it wrote.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: the call filled all of `info`; zeroed bytes are a valid `proc_taskinfo` anyway.
    let info = unsafe { info.assume_init() };
    usize::try_from(info.pti_threadnum).ok()
}
