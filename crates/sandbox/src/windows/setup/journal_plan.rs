//! The change journal's plan at set-up (S15 `PR4f`, 0049, D-B12): which volumes get the larger
//! journal and what size, apart from the calls so it is tested on every OS. The calls are in
//! `journal.rs` (Windows only).
//!
//! A volume gets a journal of [`JOURNAL_MAX`] bytes, growing by [`JOURNAL_DELTA`], when it has
//! none or a smaller one. A journal already that size or larger is left: set-up never shrinks
//! one. Only fixed NTFS volumes are chosen; a removable volume never is.

use std::path::PathBuf;

use super::{JOURNAL_DELTA, JOURNAL_MAX};

/// What set-up does to one volume's change journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalAction {
    /// Leave it: it is [`JOURNAL_MAX`] or larger.
    Leave,
    /// Create it, or enlarge it, with `FSCTL_CREATE_USN_JOURNAL`.
    Grow {
        /// `MaximumSize`, in bytes.
        max: u64,
        /// `AllocationDelta`, in bytes.
        delta: u64,
    },
}

/// The action for a volume whose journal's `MaximumSize` is `current` (`None`: no journal).
pub fn journal_plan(current: Option<u64>) -> JournalAction {
    if grows(current, JOURNAL_MAX) {
        JournalAction::Grow {
            max: JOURNAL_MAX,
            delta: JOURNAL_DELTA,
        }
    } else {
        JournalAction::Leave
    }
}

/// Whether making a journal of `max` bytes over one of `current` bytes grows it: true with no
/// journal, or one smaller than `max`. The create call checks this again just before it runs,
/// so no caller can shrink a journal.
pub fn grows(current: Option<u64>, max: u64) -> bool {
    current.is_none_or(|current| current < max)
}

/// One candidate volume, as its root's facts read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeFacts {
    /// Its root: `C:\`, or the folder it is mounted on.
    pub root: PathBuf,
    /// Whether its file system is NTFS.
    pub ntfs: bool,
    /// Whether it is on a removable drive, or a drive that isn't fixed, or one whose kind
    /// couldn't be read (the clean-close record's reading, `RootFacts::removable`).
    pub removable: bool,
}

/// The roots of `candidates` that get the journal: the NTFS volumes that aren't removable,
/// each once (roots compared without case and without a trailing separator), in order.
pub fn select_volumes(candidates: Vec<VolumeFacts>) -> Vec<PathBuf> {
    let mut chosen: Vec<PathBuf> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for volume in candidates {
        if !volume.ntfs || volume.removable {
            continue;
        }
        let key = root_key(&volume.root);
        if keys.contains(&key) {
            continue;
        }
        keys.push(key);
        chosen.push(volume.root);
    }
    chosen
}

/// `root` folded for comparison: ASCII case folded, trailing separators dropped.
fn root_key(root: &std::path::Path) -> String {
    root.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

/// The device path `CreateFileW` opens a volume by, from its volume GUID path
/// (`GetVolumeNameForVolumeMountPointW`'s `\\?\Volume{...}\`): the same without the trailing
/// backslash, which would open the volume's root folder instead. `None` for anything else.
pub fn device_path(volume_name: &str) -> Option<String> {
    let name = volume_name.trim_end_matches('\0');
    let inner = name.strip_prefix(r"\\?\Volume{")?;
    let guid = inner
        .strip_suffix(r"}\")
        .or_else(|| inner.strip_suffix('}'))?;
    let valid = !guid.is_empty() && guid.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    valid.then(|| format!(r"\\?\Volume{{{guid}}}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    fn grow() -> JournalAction {
        JournalAction::Grow {
            max: 256 * MIB,
            delta: 32 * MIB,
        }
    }

    fn volume(root: &str, ntfs: bool, removable: bool) -> VolumeFacts {
        VolumeFacts {
            root: PathBuf::from(root),
            ntfs,
            removable,
        }
    }

    #[test]
    fn the_plan_creates_grows_or_leaves_and_never_shrinks() {
        assert_eq!(JOURNAL_MAX, 256 * MIB);
        assert_eq!(JOURNAL_DELTA, 32 * MIB);
        assert_eq!(journal_plan(None), grow());
        assert_eq!(journal_plan(Some(0)), grow());
        assert_eq!(journal_plan(Some(32 * MIB)), grow());
        assert_eq!(journal_plan(Some(256 * MIB - 1)), grow());
        assert_eq!(journal_plan(Some(256 * MIB)), JournalAction::Leave);
        assert_eq!(journal_plan(Some(512 * MIB)), JournalAction::Leave);
        assert_eq!(journal_plan(Some(1024 * MIB)), JournalAction::Leave);
        assert_eq!(journal_plan(Some(u64::MAX)), JournalAction::Leave);
    }

    #[test]
    fn grows_is_false_for_any_journal_as_large_as_asked() {
        assert!(grows(None, 1));
        assert!(grows(Some(1), 2));
        assert!(!grows(Some(2), 2));
        assert!(!grows(Some(3), 2));
        // A smaller size than the journal has never counts as growing it.
        assert!(!grows(Some(256 * MIB), 32 * MIB));
    }

    #[test]
    fn only_fixed_ntfs_volumes_are_chosen_each_once() {
        let candidates = vec![
            volume(r"C:\", true, false),
            volume(r"E:\", true, true),
            volume(r"F:\", false, false),
            volume(r"G:\", false, true),
            volume(r"c:", true, false),
            volume(r"D:\Mounts\Data\", true, false),
            volume(r"d:\mounts\data", true, false),
        ];
        assert_eq!(
            select_volumes(candidates),
            [PathBuf::from(r"C:\"), PathBuf::from(r"D:\Mounts\Data\")]
        );
        assert_eq!(select_volumes(Vec::new()), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_volume_guid_path_becomes_its_device_path() {
        let guid = "1b2c3d4e-0000-1111-2222-333344445555";
        let name = format!(r"\\?\Volume{{{guid}}}\");
        let device = format!(r"\\?\Volume{{{guid}}}");
        assert_eq!(device_path(&name).as_deref(), Some(device.as_str()));
        assert_eq!(
            device_path(&format!("{name}\0")).as_deref(),
            Some(device.as_str())
        );
        assert_eq!(device_path(&device).as_deref(), Some(device.as_str()));
        assert_eq!(device_path(r"C:\"), None);
        assert_eq!(device_path(r"\\?\Volume{}\"), None);
        assert_eq!(device_path(r"\\?\Volume{..\..}\"), None);
        assert_eq!(device_path(r"\\?\Volume{1b2c}\x"), None);
    }
}
