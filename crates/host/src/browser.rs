//! The browser's host side (S27, decision record 0029): which Chrome a session runs
//! ([`chrome`]), how many run at once and the limits each gets ([`limits`]), the sites every
//! matter's allowlist starts from ([`seed_sites`]) and the background hosts the proxy denies
//! ([`deny_hosts`]), and where Chrome runs ([`BrowserLaunch`]). The `browser` entry itself is
//! filled in `agent/loads/browser.rs`; the pages it opens are recorded in `sources/browser.rs`.

pub mod chrome;
pub mod deny_hosts;
pub mod limits;
pub mod seed_sites;

/// The app-wide folder under the data folder that holds agent-browser's config file.
pub const CONFIG_DIR: &str = "browser";

/// agent-browser's config file, `<data>/browser/agent-browser.json`: the path the profile's
/// `browser` entry passes as `AGENT_BROWSER_CONFIG`
/// (`clauseo_profile::ProfilePaths::browser_config`).
pub const CONFIG_FILE: &str = "agent-browser.json";

/// Makes `<data>/browser/` (through no link below `data`, 0700 on Unix) and writes `{}` to
/// `agent-browser.json` there when it is absent, so the guard, which refuses a config path that
/// names no file, can start. An existing file is left as it is; a link or anything not a file
/// at that name is replaced. Returns the file's path. File system calls: off the actor (the
/// launch prep).
pub(crate) fn write_config(data: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    let folder = data.join(CONFIG_DIR);
    crate::write::create_private_dirs_under(data, &folder)?;
    #[cfg(unix)]
    crate::write::make_private_dir(&folder)?;
    let file = folder.join(CONFIG_FILE);
    let is_file = std::fs::symlink_metadata(&file).is_ok_and(|meta| meta.is_file());
    if !is_file {
        crate::write::replace_private_file(&file, b"{}\n")?;
    }
    Ok(file)
}

/// Where the browser runs, a switch for stage C.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BrowserLaunch {
    /// agent-browser starts Chrome itself, inside the session's jail: the only one built.
    #[default]
    InJail,
    /// Chrome runs outside the jail and the entry carries `AGENT_BROWSER_CDP` with this port
    /// instead of starting one. Not built: [`HostConfig::with_browser_launch`] refuses it.
    ///
    /// [`HostConfig::with_browser_launch`]: crate::HostConfig::with_browser_launch
    OutsideJail {
        /// The port of Chrome's `DevTools` endpoint.
        cdp_port: u16,
    },
}

/// A [`BrowserLaunch`] the host does not build yet.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "the browser outside the jail (BrowserLaunch::OutsideJail, CDP port {cdp_port}) is not built \
     yet; only BrowserLaunch::InJail is (decision record 0029)"
)]
pub struct UnsupportedLaunch {
    /// The port asked for.
    pub cdp_port: u16,
}

#[cfg(test)]
mod tests;
