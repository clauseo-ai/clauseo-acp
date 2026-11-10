//! NT device paths to the paths the lawyer knows, with no Windows call: `\Device\HarddiskVolume3\
//! Users\...` becomes `C:\Users\...` through the drive letters' devices (`QueryDosDeviceW` over
//! each letter, which `windows_trace.rs` lists). The letters are listed again on a miss, at most
//! once per [`REFRESH_EVERY`], so a drive mounted later is found and a storm of misses lists them
//! once.
//!
//! **Network paths are flagged, never made usable.** A pool user chooses the paths it opens, and
//! the runner checks a refused path's access as the lawyer (`AuthzAccessCheck`, part B): a usable
//! UNC path there would be an SMB connection, with the lawyer's credentials, to a server the agent
//! picked (an NTLM leak or relay). So every `\Device\` path that isn't under a known local volume
//! is [`Mapped::net`], with a string for display only:
//!
//! - `\Device\Mup\server\share\...`, `\Device\LanmanRedirector\;Z:<id>\server\share\...`,
//!   `\Device\WebDavRedirector\...` and any redirector form whose provider parts start with `;`
//!   (`\Device\Mup\;LanmanRedirector\;Z:<id>\...`) show as `\\server\share\...`, those parts
//!   stripped;
//! - any other device (a volume with no letter, named pipes, the console) shows as its NT path.
//!
//! A path not under `\Device\` at all is dropped.

use std::time::Duration;

/// The shortest time between two listings of the drive letters.
pub const REFRESH_EVERY: Duration = Duration::from_secs(1);
/// The NT device root.
const DEVICE: &str = r"\Device\";
/// Redirectors whose paths go on with `server\share` (after any `;` parts).
const REDIRECTORS: [&str; 4] = ["Mup", "LanmanRedirector", "WebDavRedirector", "RdpDr"];

/// A refused path as it goes on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapped {
    /// `C:\...` when local; for display only when [`Mapped::net`].
    pub path: String,
    /// Not under a known local volume: never to be opened or checked (`"net":true`).
    pub net: bool,
}

impl Mapped {
    fn local(path: String) -> Mapped {
        Mapped { path, net: false }
    }

    fn net(path: String) -> Mapped {
        Mapped { path, net: true }
    }
}

/// Each drive letter's device, as last listed.
#[derive(Debug, Clone, Default)]
pub struct DeviceMap {
    drives: Vec<(char, String)>,
    listed_at: Option<Duration>,
}

impl DeviceMap {
    /// `nt_path` with its drive letter, or flagged as network, listing the letters again
    /// through `devices` on a miss when [`REFRESH_EVERY`] has passed since the last listing at
    /// `now`. `None` for a path not under `\Device\`.
    pub fn map_path(
        &mut self,
        nt_path: &str,
        now: Duration,
        devices: &mut dyn FnMut() -> Vec<(char, String)>,
    ) -> Option<Mapped> {
        strip_prefix_folded(nt_path, DEVICE)?;
        if let Some(rest) = redirected(nt_path) {
            return Some(Mapped::net(
                share_display(rest).unwrap_or_else(|| nt_path.to_owned()),
            ));
        }
        if let Some(path) = self.lookup(nt_path) {
            return Some(Mapped::local(path));
        }
        let due = self
            .listed_at
            .is_none_or(|at| now.saturating_sub(at) >= REFRESH_EVERY);
        if due {
            self.drives = devices();
            self.listed_at = Some(now);
            if let Some(path) = self.lookup(nt_path) {
                return Some(Mapped::local(path));
            }
        }
        Some(Mapped::net(nt_path.to_owned()))
    }

    /// `nt_path` through the letters as listed, local volumes only.
    fn lookup(&self, nt_path: &str) -> Option<String> {
        self.drives.iter().find_map(|(letter, device)| {
            if !letter.is_ascii_alphabetic()
                || strip_prefix_folded(device, DEVICE).is_none()
                || redirected(device).is_some()
            {
                return None;
            }
            let rest = strip_prefix_folded(nt_path, device.trim_end_matches('\\'))?;
            if rest.is_empty() {
                Some(format!("{letter}:\\"))
            } else if rest.starts_with('\\') {
                Some(format!("{letter}:{rest}"))
            } else {
                None
            }
        })
    }
}

/// For a redirector's path, what follows its device and any `;` parts: `server\share\...`.
/// A device not in [`REDIRECTORS`] counts when a `;` part follows it (`\;Z:<id>\`).
fn redirected(nt_path: &str) -> Option<&str> {
    let rest = strip_prefix_folded(nt_path, DEVICE)?;
    let (device, mut after) = rest.split_once('\\')?;
    let mut provider = false;
    while after.starts_with(';') {
        provider = true;
        after = after.split_once('\\').map_or("", |(_, next)| next);
    }
    let known = REDIRECTORS
        .iter()
        .any(|name| device.eq_ignore_ascii_case(name));
    (known || provider).then_some(after)
}

/// `server\share\...` as `\\server\share\...`, for display: `None` without both.
fn share_display(rest: &str) -> Option<String> {
    let mut parts = rest.split('\\');
    let (server, share) = (parts.next()?, parts.next()?);
    (!server.is_empty() && !share.is_empty()).then(|| format!(r"\\{rest}"))
}

/// `text` after `prefix`, compared with ASCII case folded, as NT device names are.
fn strip_prefix_folded<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| text.get(prefix.len()..))
        .flatten()
}
