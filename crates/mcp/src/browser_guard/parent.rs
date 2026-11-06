//! Notices that the process that started the guard (the agent, or whatever stands in for it)
//! has gone, even when nothing closes the guard's stdin.
//!
//! [`Parent::now`] runs first thing in [`super::run`], before anything slow, so a parent that
//! dies while Chrome starts is still the one watched. On Unix it reads the parent pid: once the
//! parent dies the guard is reparented and the pid changes; a parent pid of 1 at the start
//! means the parent is gone already (no Clauseo setup starts the guard from pid 1). On Windows
//! it opens a handle to the parent process (`parent/windows.rs`), which is signalled when the
//! parent exits, however late the guard waits on it. A parent already gone at the start makes
//! the guard exit 1 at once. While Chrome starts, `chrome.rs` asks [`Parent::gone`] every
//! [`POLL`]; afterwards a thread ([`Parent::watch`]) reports it to the relay, which ends
//! agent-browser, and the guard ends Chrome and the daemon and exits 1.

use std::time::Duration;

/// How often the Unix parent pid is read, and how often Chrome's start looks at the parent.
pub(crate) const POLL: Duration = Duration::from_millis(250);

/// The guard's parent as it was when the guard started.
#[derive(Clone)]
pub(crate) struct Parent {
    #[cfg(unix)]
    pid: u32,
    /// `None` when the parent cannot be watched (Windows refused the handle); only stdin's end
    /// stops the guard then.
    #[cfg(windows)]
    handle: Option<std::sync::Arc<windows::Parent>>,
}

impl Parent {
    /// The guard's parent now, or `None` if it has gone already.
    pub(crate) fn now() -> Option<Parent> {
        #[cfg(unix)]
        {
            let pid = std::os::unix::process::parent_id();
            (!gone_unix(pid, pid)).then_some(Parent { pid })
        }
        #[cfg(windows)]
        {
            match windows::parent() {
                windows::Lookup::Found(parent) => Some(Parent {
                    handle: Some(std::sync::Arc::new(parent)),
                }),
                windows::Lookup::Gone => None,
                windows::Lookup::Unknown => {
                    super::note(
                        "cannot watch the guard's parent; only stdin's end will stop the guard",
                    );
                    Some(Parent { handle: None })
                }
            }
        }
    }

    /// Whether the parent has gone since [`Parent::now`].
    pub(crate) fn gone(&self) -> bool {
        #[cfg(unix)]
        {
            gone_unix(self.pid, std::os::unix::process::parent_id())
        }
        #[cfg(windows)]
        {
            self.handle.as_ref().is_some_and(|parent| parent.exited())
        }
    }

    /// Calls `on_gone` once, from a thread, when the parent has gone (at once if it has gone
    /// already).
    pub(crate) fn watch(&self, on_gone: impl FnOnce() + Send + 'static) {
        #[cfg(unix)]
        {
            let parent = self.clone();
            drop(std::thread::spawn(move || {
                while !parent.gone() {
                    super::pause(POLL);
                }
                on_gone();
            }));
        }
        #[cfg(windows)]
        {
            let Some(parent) = self.handle.clone() else {
                return;
            };
            drop(std::thread::spawn(move || {
                if parent.wait() {
                    on_gone();
                }
            }));
        }
    }
}

/// Whether a guard whose parent was `first` at its start and is `now` has lost it: reparented,
/// or started already orphaned (pid 1).
#[cfg(unix)]
pub(crate) fn gone_unix(first: u32, now: u32) -> bool {
    first <= 1 || now != first
}

#[cfg(windows)]
pub(crate) mod windows;

#[cfg(all(test, unix))]
mod tests {
    use super::gone_unix;

    #[test]
    fn a_parent_pid_of_one_or_a_new_one_is_a_gone_parent() {
        assert!(!gone_unix(4242, 4242));
        assert!(gone_unix(4242, 1));
        assert!(gone_unix(4242, 977), "a subreaper adopted the guard");
        assert!(gone_unix(1, 1), "orphaned before the guard ran");
    }
}
