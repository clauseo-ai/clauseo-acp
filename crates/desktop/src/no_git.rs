//! The `git` that does nothing, at each app start: the bundled `clauseo-mcp` (beside the app's
//! executable) copied to `<data>/tools/no-git/git` (`git.exe` on Windows) when the copy is
//! missing or differs (`clauseo_profile::tools::install_no_git`), and named to the host with its
//! SHA-256 (`HostConfig::with_no_git`), which re-checks it at every agent launch and refuses the
//! launch when it is missing or changed. A copy that failed still names the program, with no
//! hash, so the host refuses rather than run an agent without it; the next start copies again.
//! Off the host thread, on the host-start thread. Logs hold a word, never a path.

use std::path::{Path, PathBuf};

use clauseo_host::{HostConfig, NoGit};
use clauseo_settings::Locations;

/// The log target of the copy's outcome, for support.
const LIFECYCLE: &str = "clauseo_desktop::no_git::lifecycle";

/// `config` with the `git` that does nothing, copied from beside this executable.
pub(crate) fn with_no_git(config: HostConfig, locations: &Locations) -> HostConfig {
    with_no_git_from(config, locations, std::env::current_exe().ok())
}

/// [`with_no_git`] for the app's executable at `exe` (`None`: not found).
pub(crate) fn with_no_git_from(
    config: HostConfig,
    locations: &Locations,
    exe: Option<PathBuf>,
) -> HostConfig {
    let paths = exe.and_then(|exe| crate::profile::paths_for(locations, exe, None).ok());
    let Some(paths) = paths else {
        tracing::warn!(
            target: LIFECYCLE,
            outcome = "no_paths",
            "the agents' git program was not copied: agents will not start"
        );
        let program = clauseo_profile::paths::no_git_command(locations.data_dir());
        return config.with_no_git(NoGit::new(program, None));
    };
    let program = paths.no_git_command();
    let source: &Path = &paths.browser().clauseo_mcp;
    let sha256 = match clauseo_profile::tools::install_no_git(source, &paths) {
        Ok((installed, sha256)) => {
            tracing::info!(
                target: LIFECYCLE,
                outcome = installed.word(),
                "the agents' git program is in place"
            );
            Some(sha256)
        }
        Err(err) => {
            tracing::warn!(
                target: LIFECYCLE,
                outcome = "failed",
                err = %err.log_text(),
                "the agents' git program was not copied: agents will not start"
            );
            None
        }
    };
    config.with_no_git(NoGit::new(program, sha256))
}

#[cfg(test)]
mod tests;
