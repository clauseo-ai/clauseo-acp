//! Whether the listening socket on `127.0.0.1:<port>` belongs to our own login's process tree,
//! asked before the `GET /cancel` ([`super::send_cancel`]): `/cancel` is not scoped to Clauseo's
//! login, so it goes only to a listener the login itself holds.
//!
//! - Linux: the `LISTEN` sockets on the port from `/proc/net/tcp` and `/proc/net/tcp6`, by
//!   inode, each matched to a `socket:[inode]` link under `/proc/<pid>/fd` of a pid in the tree.
//! - macOS (and any Unix without `/proc`): `lsof -nP -iTCP:<port> -sTCP:LISTEN -Fp`, started
//!   through the caller's launcher with an empty environment and a 500 ms limit, its `p<pid>`
//!   lines.
//! - Windows: `GetExtendedTcpTable` with `TCP_TABLE_OWNER_PID_LISTENER`, IPv4 and IPv6
//!   ([`windows`]).
//!
//! Owned means: at least one listener on the port, and every listener on it in the tree. Any
//! failure to tell (a table that cannot be read, `lsof` missing) is "not owned", so nothing is
//! sent and the run is killed at the bound instead.

use std::collections::BTreeSet;

use clauseo_process::{Launcher, Timer};

/// Whether every listener on `port` belongs to a pid in `ours`, and there is one.
pub(crate) async fn owns_listener<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    port: u16,
    ours: &[u32],
) -> bool {
    let owners = listener_pids(launcher, timer, port).await;
    owners.is_some_and(|owners| owned(&owners, ours))
}

/// The rule: some listener, and none outside `ours`.
pub(crate) fn owned(owners: &BTreeSet<u32>, ours: &[u32]) -> bool {
    !owners.is_empty() && owners.iter().all(|pid| ours.contains(pid))
}

/// The pids holding a listener on `port`, or `None` when that cannot be told. On Linux a
/// listener no readable process shows (another user's) counts as pid 0, "someone else".
#[cfg(unix)]
async fn listener_pids<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    port: u16,
) -> Option<BTreeSet<u32>> {
    if let Some(inodes) = proc_net::listen_inodes(port) {
        return Some(proc_net::owners(&inodes));
    }
    lsof::listener_pids(launcher, timer, port).await
}

#[cfg(windows)]
async fn listener_pids<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    port: u16,
) -> Option<BTreeSet<u32>> {
    let _ = (launcher, timer);
    // The table is read at once; the await keeps one signature for every OS.
    futures::future::ready(windows::listener_pids(port)).await
}

#[cfg(not(any(unix, windows)))]
async fn listener_pids<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    port: u16,
) -> Option<BTreeSet<u32>> {
    let _ = (launcher, timer, port);
    futures::future::ready(None).await
}

/// Linux's tables.
#[cfg(unix)]
pub(crate) mod proc_net {
    use std::collections::BTreeSet;
    use std::path::Path;

    /// The kernel's state number for `LISTEN`.
    const LISTEN: &str = "0A";

    /// The inodes of every `LISTEN` socket on `port`, from `/proc/net/tcp` and `tcp6`; `None`
    /// when neither can be read (no `/proc`).
    pub(crate) fn listen_inodes(port: u16) -> Option<BTreeSet<u64>> {
        let mut read_any = false;
        let mut inodes = BTreeSet::new();
        for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
            if let Ok(text) = std::fs::read_to_string(table) {
                read_any = true;
                inodes.extend(parse_listen(&text, port));
            }
        }
        read_any.then_some(inodes)
    }

    /// The inodes of the `LISTEN` rows on `port` in one table's text.
    pub(crate) fn parse_listen(text: &str, port: u16) -> Vec<u64> {
        text.lines()
            .skip(1)
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                let local = fields.get(1)?;
                let state = fields.get(3)?;
                let inode = fields.get(9)?.parse::<u64>().ok()?;
                let (_, hex_port) = local.rsplit_once(':')?;
                let row_port = u16::from_str_radix(hex_port, 16).ok()?;
                (*state == LISTEN && row_port == port && inode != 0).then_some(inode)
            })
            .collect()
    }

    /// For each inode, the pid among the processes under `/proc` whose descriptors hold it; an
    /// inode no process shows (another user's, or gone) counts as pid 0.
    pub(crate) fn owners(inodes: &BTreeSet<u64>) -> BTreeSet<u32> {
        let mut owners = BTreeSet::new();
        let mut left = inodes.clone();
        if let Ok(processes) = std::fs::read_dir("/proc") {
            for process in processes.flatten() {
                if left.is_empty() {
                    break;
                }
                let Some(pid) = process
                    .file_name()
                    .to_str()
                    .and_then(|name| name.parse::<u32>().ok())
                else {
                    continue;
                };
                let held = socket_inodes(&process.path().join("fd"));
                let found: Vec<u64> = left.intersection(&held).copied().collect();
                if !found.is_empty() {
                    owners.insert(pid);
                    for inode in found {
                        left.remove(&inode);
                    }
                }
            }
        }
        if !left.is_empty() {
            owners.insert(0);
        }
        owners
    }

    /// The socket inodes in one process's `fd` folder (`socket:[inode]` links).
    fn socket_inodes(fd: &Path) -> BTreeSet<u64> {
        let Ok(entries) = std::fs::read_dir(fd) else {
            return BTreeSet::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let target = std::fs::read_link(entry.path()).ok()?;
                let text = target.to_str()?;
                text.strip_prefix("socket:[")?
                    .strip_suffix(']')?
                    .parse::<u64>()
                    .ok()
            })
            .collect()
    }
}

/// `lsof`, where there is no `/proc` (macOS).
#[cfg(unix)]
pub(crate) mod lsof {
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::pin::pin;
    use std::time::Duration;

    use clauseo_process::{LaunchSpec, Launched, Launcher, Timer};
    use futures::future::{Either, join3, select};
    use futures::io::{AsyncReadExt as _, AsyncWriteExt as _};

    /// Where `lsof` is: macOS's, then a Linux one.
    const PLACES: [&str; 2] = ["/usr/sbin/lsof", "/usr/bin/lsof"];
    /// The most `lsof` may take; past it the listener counts as not owned, so the stop goes on
    /// to the bound without a `/cancel`.
    const LIMIT: Duration = Duration::from_millis(500);
    /// The most of its output read.
    const OUTPUT_MAX: u64 = 64 * 1024;

    /// The pids `lsof` names as holding a listener on `port`; `None` when it is not there, did
    /// not start, or did not end within [`LIMIT`].
    pub(crate) async fn listener_pids<T: Timer>(
        launcher: &dyn Launcher,
        timer: &T,
        port: u16,
    ) -> Option<BTreeSet<u32>> {
        let program = PLACES.iter().map(Path::new).find(|path| path.is_file())?;
        let args = vec![
            OsString::from("-nP"),
            OsString::from(format!("-iTCP:{port}")),
            OsString::from("-sTCP:LISTEN"),
            OsString::from("-Fp"),
        ];
        let spec = LaunchSpec::new(
            program.to_path_buf(),
            args,
            BTreeMap::new(),
            PathBuf::from("/"),
        )
        .ok()?;
        let Launched {
            mut stdin,
            stdout,
            stderr,
            mut handle,
            ..
        } = launcher.launch(&spec).ok()?;
        drop(stdin.close().await);
        let ran = pin!(async {
            let mut text = String::new();
            let read = async {
                drop(stdout.take(OUTPUT_MAX).read_to_string(&mut text).await);
            };
            let ((), drained, _) = join3(read, stderr.drain(), handle.exited()).await;
            drop(drained);
            text
        });
        let limit = pin!(timer.after(LIMIT));
        match select(ran, limit).await {
            Either::Left((text, _)) => Some(parse_pids(&text)),
            Either::Right(_) => None,
        }
    }

    /// The pids in `lsof -Fp`'s `p<pid>` lines.
    pub(crate) fn parse_pids(text: &str) -> BTreeSet<u32> {
        text.lines()
            .filter_map(|line| line.strip_prefix('p')?.trim().parse::<u32>().ok())
            .collect()
    }
}

#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests;
