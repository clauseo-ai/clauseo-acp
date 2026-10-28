//! Waits, without polling, for a process the test did not start itself (the agent's grandchild)
//! to exit, and kills it if the test fails first.
//!
//! - Linux: the test process is a child subreaper ([`before_launch`]), so an orphaned grandchild
//!   becomes its child, and [`Watch::wait`] reaps children with a blocking `waitpid` until it
//!   reaps the grandchild. From the agent's exit on, the test process always has a child (the
//!   agent's zombie, or the grandchild the kernel handed it in the same step), so `waitpid` never
//!   finds none.
//! - macOS: a kqueue `EVFILT_PROC` / `NOTE_EXIT` registration, made while the process is alive.
//! - Windows: `OpenProcess(SYNCHRONIZE)`, then `WaitForSingleObject(INFINITE)`.
//!
//! A survivor blocks the wait until nextest's slow-timeout ends the test.

/// Makes this test process a child subreaper (Linux), before it launches the agent.
pub(crate) fn before_launch() {
    #[cfg(target_os = "linux")]
    nix::sys::prctl::set_child_subreaper(true).expect("become a child subreaper");
}

/// A watch on a process the test did not start. Dropped without [`Watch::wait`], as when a test
/// fails, it kills the process, so no `--hold` grandchild outlives a failed test.
pub(crate) struct Watch {
    pid: u32,
    os: Option<os::Os>,
}

impl Watch {
    /// Starts watching `pid`, which must still be running.
    pub(crate) fn new(pid: u32) -> Watch {
        Watch {
            pid,
            os: Some(os::watch(pid)),
        }
    }

    /// Blocks until the process exits.
    pub(crate) fn wait(mut self) {
        println!("grandchild {}", self.pid);
        if let Some(os) = self.os.take() {
            os::wait(&os, self.pid);
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if let Some(os) = &self.os {
            os::kill_watched(os, self.pid);
        }
    }
}

/// Kills process `pid` alone: not its group, not its job.
pub(crate) fn kill_only(pid: u32) {
    os::kill_only(pid);
}

#[cfg(unix)]
fn pid(pid: u32) -> nix::unistd::Pid {
    nix::unistd::Pid::from_raw(i32::try_from(pid).unwrap())
}

/// SIGKILL to `pid` alone.
#[cfg(unix)]
fn sigkill(pid: u32) -> nix::Result<()> {
    nix::sys::signal::kill(self::pid(pid), nix::sys::signal::Signal::SIGKILL)
}

#[cfg(target_os = "linux")]
mod os {
    use nix::sys::wait::waitpid;
    use nix::unistd::Pid;

    pub(super) struct Os;

    pub(super) fn watch(pid: u32) -> Os {
        nix::sys::signal::kill(super::pid(pid), None).expect("the watched process is running");
        Os
    }

    pub(super) fn wait(_os: &Os, pid: u32) {
        let pid = super::pid(pid);
        loop {
            let status = waitpid(Pid::from_raw(-1), None).expect("waitpid");
            if status.pid() == Some(pid) {
                return;
            }
        }
    }

    pub(super) fn kill_watched(_os: &Os, pid: u32) {
        super::sigkill(pid).unwrap_or_default();
    }

    pub(super) fn kill_only(pid: u32) {
        super::sigkill(pid).expect("kill the process");
    }
}

#[cfg(target_os = "macos")]
mod os {
    use nix::sys::event::{EvFlags, EventFilter, FilterFlag, KEvent, Kqueue};

    pub(super) struct Os(Kqueue);

    fn exit_event(pid: u32, flags: EvFlags) -> KEvent {
        let ident = usize::try_from(pid).unwrap();
        KEvent::new(
            ident,
            EventFilter::EVFILT_PROC,
            flags,
            FilterFlag::NOTE_EXIT,
            0,
            0,
        )
    }

    /// `ESRCH` here means the process is already gone, which the caller says it is not.
    pub(super) fn watch(pid: u32) -> Os {
        let queue = Kqueue::new().expect("kqueue");
        let add = exit_event(pid, EvFlags::EV_ADD | EvFlags::EV_ONESHOT);
        queue
            .kevent(&[add], &mut [], None)
            .expect("the watched process is running");
        Os(queue)
    }

    pub(super) fn wait(os: &Os, pid: u32) {
        let mut events = [exit_event(pid, EvFlags::empty())];
        let n = os.0.kevent(&[], &mut events, None).expect("kevent");
        assert_eq!(n, 1);
        assert_eq!(events[0].ident(), usize::try_from(pid).unwrap());
    }

    pub(super) fn kill_watched(_os: &Os, pid: u32) {
        super::sigkill(pid).unwrap_or_default();
    }

    pub(super) fn kill_only(pid: u32) {
        super::sigkill(pid).expect("kill the process");
    }
}

#[cfg(windows)]
mod os {
    #![expect(
        unsafe_code,
        reason = "FFI to OpenProcess, WaitForSingleObject and TerminateProcess, for the tests"
    )]

    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
        WaitForSingleObject,
    };

    pub(super) struct Os(HANDLE);

    impl Drop for Os {
        fn drop(&mut self) {
            // SAFETY: `self.0` is a handle this watch owns and closes only here.
            drop(unsafe { CloseHandle(self.0) });
        }
    }

    pub(super) fn watch(pid: u32) -> Os {
        // SAFETY: opening a process by id takes no pointer; `Os` closes the handle.
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, false, pid) }
            .expect("the watched process is running");
        Os(process)
    }

    pub(super) fn wait(os: &Os, _pid: u32) {
        // SAFETY: `os.0` is a live process handle opened with SYNCHRONIZE.
        let waited = unsafe { WaitForSingleObject(os.0, INFINITE) };
        assert_eq!(waited, WAIT_OBJECT_0);
    }

    pub(super) fn kill_watched(os: &Os, _pid: u32) {
        // SAFETY: `os.0` is a live process handle opened with PROCESS_TERMINATE.
        drop(unsafe { TerminateProcess(os.0, 1) });
    }

    pub(super) fn kill_only(pid: u32) {
        let os = watch(pid);
        // SAFETY: as in `kill_watched`.
        unsafe { TerminateProcess(os.0, 1) }.expect("kill the process");
    }
}
