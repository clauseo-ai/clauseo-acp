//! Whether a session's processes are gone: the one check the host's waits ask before they free
//! what a session held or launch its successor under another policy
//! (`launch_checks::straggler`, `launch_checks::relaunch` in clauseo-host).

use clauseo_process::TreeWatch;

/// Whether every process of the session whose agent's tree `tree` watches is gone. A
/// blocking-free query; the host polls it, with its own cap.
///
/// Today it is the tree's own check (`TreeWatch::gone`: on Unix the agent reaped and its
/// process group empty, on Windows its Job Object empty). Known gap (decision record 0051): a
/// process that calls `setsid` or `setpgid` (macOS, Linux), or was reparented to launchd or
/// init outside the group, is not seen, keeps its launch's policy and outlives the check.
/// Stage C's fix ("find and end every process of a macOS session, including setsid escapees
/// and orphans") goes here, so the host's waits need no change.
pub fn session_processes_gone(tree: &TreeWatch) -> bool {
    tree.gone()
}
