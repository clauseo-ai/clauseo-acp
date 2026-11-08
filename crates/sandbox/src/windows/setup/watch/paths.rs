//! NT device paths to the paths the lawyer knows, with no Windows call: `\Device\HarddiskVolume3\
//! Users\...` becomes `C:\Users\...` through the drive letters' devices (`QueryDosDeviceW` over
//! each letter, which `windows_trace.rs` lists), and `\Device\Mup\server\share\...` becomes
//! `\\server\share\...`. Any other device (named pipes, the console, a volume with no letter) is
//! dropped, as macOS drops `/dev`. The letters are listed again on a miss, at most once per
//! [`REFRESH_EVERY`], so a drive mounted later is found and a storm of misses lists them once.

use std::time::Duration;

/// The shortest time between two listings of the drive letters.
pub const REFRESH_EVERY: Duration = Duration::from_secs(1);
/// The network redirector's device.
const MUP: &str = r"\Device\Mup\";

/// Each drive letter's device, as last listed.
#[derive(Debug, Clone, Default)]
pub struct DeviceMap {
    drives: Vec<(char, String)>,
    listed_at: Option<Duration>,
}

impl DeviceMap {
    /// `nt_path` with its drive letter (or as a UNC path), listing the letters again through
    /// `devices` on a miss when [`REFRESH_EVERY`] has passed since the last listing at `now`.
    pub fn map_path(
        &mut self,
        nt_path: &str,
        now: Duration,
        devices: &mut dyn FnMut() -> Vec<(char, String)>,
    ) -> Option<String> {
        if let Some(rest) = strip_prefix_folded(nt_path, MUP) {
            return (!rest.is_empty()).then(|| format!(r"\\{rest}"));
        }
        if let Some(path) = self.lookup(nt_path) {
            return Some(path);
        }
        let due = self
            .listed_at
            .is_none_or(|at| now.saturating_sub(at) >= REFRESH_EVERY);
        if !due {
            return None;
        }
        self.drives = devices();
        self.listed_at = Some(now);
        self.lookup(nt_path)
    }

    /// `nt_path` through the letters as listed.
    fn lookup(&self, nt_path: &str) -> Option<String> {
        self.drives.iter().find_map(|(letter, device)| {
            if !letter.is_ascii_alphabetic() || strip_prefix_folded(device, r"\Device\").is_none() {
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

/// `text` after `prefix`, compared with ASCII case folded, as NT device names are.
fn strip_prefix_folded<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| text.get(prefix.len()..))
        .flatten()
}
