//! The escaper the session sweep tests (`sweep.rs`) run in the jail: this test binary, run with
//! `--exact escaper::escaper`, forks a child that calls `setsid` and forks the escaped process,
//! then exits, so launchd adopts it. No shell can call `setsid`, so the tests depend on nothing
//! but `/bin/sh` and this binary.

use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

/// The folder the escaper writes in: `pids` (one pid a line) and `ticks` (a line each 100 ms).
pub(crate) const ESCAPE_DIR: &str = "CLAUSEO_SWEEP_ESCAPE_DIR";
/// How many 20 s sleepers the escaped process starts in its own session (none if unset).
pub(crate) const ESCAPE_FANOUT: &str = "CLAUSEO_SWEEP_ESCAPE_FANOUT";

/// Run in the jail by the tests below, never on its own: forks a child that calls `setsid` and
/// forks the escaped process, then exits, so launchd adopts it. The escaped process closes every
/// descriptor, logs its pid, starts `ESCAPE_FANOUT` sleepers (each logging its pid), then ticks
/// every 100 ms for 60 s.
#[test]
#[ignore = "run by the sweep tests inside the jail"]
fn escaper() {
    let Some(dir) = std::env::var_os(ESCAPE_DIR) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let c = |name: &str| CString::new(dir.join(name).into_os_string().into_vec()).unwrap();
    let fanout: u32 = std::env::var(ESCAPE_FANOUT)
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    escape(&c("pids"), &c("ticks"), fanout);
}

#[expect(
    unsafe_code,
    reason = "fork, setsid, close and _exit: the escape a jailed tool can make, for the sweep tests"
)]
fn escape(pids: &CStr, ticks: &CStr, fanout: u32) {
    // SAFETY: after each fork the child calls only async-signal-safe functions (setsid, fork,
    // close, getpid, open, write, usleep, _exit) on buffers made before the fork, so the
    // other threads of the test harness, absent in the child, hold nothing it needs.
    unsafe {
        let child = libc::fork();
        if child == 0 {
            libc::setsid();
            if libc::fork() == 0 {
                for fd in 0..1024 {
                    libc::close(fd);
                }
                append_pid(pids);
                for _ in 0..fanout {
                    if libc::fork() == 0 {
                        append_pid(pids);
                        for _ in 0..200 {
                            libc::usleep(100_000);
                        }
                        libc::_exit(0);
                    }
                }
                for _ in 0..600 {
                    append(ticks, b"x\n");
                    libc::usleep(100_000);
                }
                libc::_exit(0);
            }
            libc::_exit(0);
        }
        let mut status = 0;
        libc::waitpid(child, &raw mut status, 0);
    }
}

/// Appends this process's pid and a newline to `path`, with no allocation.
#[expect(unsafe_code, reason = "getpid in a forked child, for the sweep tests")]
fn append_pid(path: &CStr) {
    // SAFETY: getpid(2) takes nothing and cannot fail.
    let mut pid = unsafe { libc::getpid() }.unsigned_abs();
    let mut digits = [0u8; 12];
    let mut at = digits.len() - 1;
    digits[at] = b'\n';
    loop {
        at -= 1;
        digits[at] = b'0' + u8::try_from(pid % 10).unwrap_or(0);
        pid /= 10;
        if pid == 0 {
            break;
        }
    }
    append(path, &digits[at..]);
}

/// Appends `bytes` to `path`, made if missing, with no allocation.
#[expect(
    unsafe_code,
    reason = "open, write and close in a forked child, for the sweep tests"
)]
fn append(path: &CStr, bytes: &[u8]) {
    // SAFETY: `path` is NUL-terminated; `bytes` is live for the write; the descriptor is this
    // call's and closed here.
    unsafe {
        let fd = libc::open(
            path.as_ptr(),
            libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT,
            0o600,
        );
        if fd >= 0 {
            libc::write(fd, bytes.as_ptr().cast(), bytes.len());
            libc::close(fd);
        }
    }
}
