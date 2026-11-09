//! Linux: Chrome's children from `/proc` (`postcheck.rs`): the tree from each process's
//! `stat`, its arguments from `cmdline`, its seccomp mode from `status` and its namespaces from
//! `ns/pid` and `ns/user`. Plain file reads; what they mean is in `judge.rs`.

use std::fs;

use super::judge::{self, Namespaces};
use super::{Source, descendants};

/// Reads `/proc` afresh at each call.
pub(super) struct Procfs;

impl Source for Procfs {
    fn below(&mut self, chrome: u32) -> Option<Vec<u32>> {
        let mut pairs = Vec::new();
        for entry in fs::read_dir("/proc").ok()?.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse().ok())
            else {
                continue;
            };
            // Gone since the listing: not below Chrome any more.
            if let Some(parent) = fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|stat| parent_in(&stat))
            {
                pairs.push((pid, parent));
            }
        }
        Some(descendants(chrome, &pairs))
    }

    fn args(&mut self, pid: u32) -> Option<Vec<u8>> {
        fs::read(format!("/proc/{pid}/cmdline")).ok()
    }

    fn sandboxed(&mut self, chrome: u32, pid: u32) -> Result<(), &'static str> {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).ok();
        judge::linux(status.as_deref(), &namespaces(chrome), &namespaces(pid))
    }
}

/// The parent pid in a `/proc/<pid>/stat` line: the second field after the command name, which
/// is in parentheses and may hold spaces and parentheses itself, so read after the last `)`.
pub(super) fn parent_in(stat: &str) -> Option<u32> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_ascii_whitespace().nth(1)?.parse().ok()
}

/// `pid`'s PID and user namespaces.
fn namespaces(pid: u32) -> Namespaces {
    let link = |name: &str| {
        fs::read_link(format!("/proc/{pid}/ns/{name}"))
            .ok()
            .map(|target| target.to_string_lossy().into_owned())
    };
    Namespaces {
        pid: link("pid"),
        user: link("user"),
    }
}
