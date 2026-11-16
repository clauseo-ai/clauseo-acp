//! Which matter paths a turn wrote, as the after-turn checkpoint and its repair take them: by
//! its tool calls ([`tool_paths`]) and by its shell commands ([`writes_in`]).

use std::collections::BTreeSet;
use std::path::Path;

use clauseo_matters::matter_path_of_resolved;
use clauseo_protocol::MatterPath;

use crate::network_paths::resolve::Disk;
use crate::network_paths::{self, NetEnv};
use crate::sent::{Places, TurnPaths};
use crate::shell_writes;

/// The matter paths a turn's tool calls wrote ([`TurnPaths::written`]), in `folder`: those
/// `acp` placed inside the session folder by text, and each `outside` path that the network-safe
/// walk resolves into `folder` on the disk (a spelling through a symlink, or in another case
/// where the file system ignores case). The rest lie outside the matter, and so does a path the
/// network-safe walk does not vouch for, or one in a matter kept on a share, which is never
/// resolved ([`network_paths::touchable`]; its walked path compared by
/// [`matter_path_of_resolved`]).
pub(crate) fn tool_paths(folder: &Path, paths: &TurnPaths) -> BTreeSet<MatterPath> {
    let Places {
        mut inside,
        outside,
    } = paths.written();
    let net = NetEnv::current();
    let disk = Disk::resolved(&net);
    for path in outside {
        // The walked path, compared as it is: the agent's text is never resolved again.
        let Some(walked) = network_paths::touchable(Path::new(&path), folder, &disk) else {
            continue;
        };
        if let Some(found) = matter_path_of_resolved(folder, &walked) {
            inside.insert(found);
        }
    }
    inside
}

/// The files the turn's shell commands write that lie in `folder`, as paths relative to it: by
/// text first ([`shell_writes::in_matter`]), else, for an absolute target, where the
/// network-safe walk leads, compared with the folder as [`tool_paths`] compares an `outside`
/// place. So a spelling through a symlink (macOS's `/var` for `/private/var`, a matter under a
/// linked cloud folder) is the matter's file, a file not made yet resolves through
/// its folder, and a target in a matter kept on a share is never resolved.
pub(super) fn writes_in(folder: &Path, writes: &BTreeSet<String>) -> BTreeSet<String> {
    let net = NetEnv::current();
    let disk = Disk::resolved(&net);
    writes
        .iter()
        .filter_map(|target| {
            shell_writes::in_matter(folder, target).or_else(|| {
                let walked = network_paths::touchable(Path::new(target), folder, &disk)?;
                matter_path_of_resolved(folder, &walked).map(|path| path.as_str().to_owned())
            })
        })
        .collect()
}
