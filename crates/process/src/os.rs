//! The OS half of an agent's process tree: a process group on Unix, a Job Object on Windows.
//! Every OS call of the crate sits here.
//!
//! Each half has `prepare`, which sets up the command before it starts, and a `Tree`, which
//! `adopt` builds from the started child and which ends the tree with `kill` (and, on Unix only,
//! asks it to stop with `terminate`). Neither waits. Each half also has a `Root`, which lists
//! the agent's descendants (`linux.rs` and `macos.rs` read them for Unix; Windows reads the job's
//! process list), and the call that ends one listed process once its start is checked again.
//! And each has `watch`, whose `Watch` asks from any thread whether the tree is gone.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(test, target_os = "macos"))]
mod name_match;
#[cfg(target_os = "macos")]
pub(crate) use macos::named;
#[cfg(any(test, target_os = "macos"))]
pub(crate) mod procargs;
#[cfg(all(test, target_os = "linux"))]
pub(crate) use linux::uids as linux_uids;

/// A session's sweep (`session.rs`) over this machine's processes, as Linux reads them.
#[cfg(target_os = "linux")]
pub(crate) async fn sweep_marked(entry: &[u8], timer: &impl crate::Timer) -> crate::SessionSwept {
    crate::session::passes::sweep(&linux::Procfs::new(), entry, timer).await
}

/// A session's sweep (`session.rs`) over this machine's processes, as macOS reads them.
#[cfg(target_os = "macos")]
pub(crate) async fn sweep_marked(entry: &[u8], timer: &impl crate::Timer) -> crate::SessionSwept {
    crate::session::passes::sweep(&macos::Marked::new(), entry, timer).await
}

/// No session sweep on this OS: on Windows the agent's Job Object takes every process it started.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn sweep_marked(
    _entry: &[u8],
    _timer: &impl crate::Timer,
) -> std::future::Ready<crate::SessionSwept> {
    std::future::ready(crate::SessionSwept::NotNeeded)
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{Ask, Root, Tree, Watch, now, prepare, signal, watch};
#[cfg(all(unix, feature = "test-support"))]
pub(crate) use unix::{kill_pid, running, started_before_now};

#[cfg(windows)]
mod win32;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::watch::{Watch, watch};
#[cfg(windows)]
pub(crate) use windows::{Root, Tree, named, now, prepare, terminate};
#[cfg(all(windows, feature = "test-support"))]
pub(crate) use windows::{image_and_parent, kill_pid, running, started_before_now};
