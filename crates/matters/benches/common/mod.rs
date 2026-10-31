//! What the matters benches share: their file writes, a seeded generator of file contents, the
//! checkpoint ids and the import of a generated folder.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use clauseo_matters::{
    DEFAULT_MARKER, Imported, Limits, Matter, NewMatter, OsProbe, Retention, Settings,
};
use clauseo_protocol::{CheckpointId, Timestamp};

/// 2020-01-01T00:00:00Z: every generated file's modification time.
pub(crate) const BASE_MTIME: u64 = 1_577_836_800;

/// The file system writes the bench's setup makes; the crate's own go through its write module.
#[expect(
    clippy::disallowed_methods,
    reason = "the matters benches generate their folders with std's writes here alone"
)]
pub(crate) mod fs_setup {
    use std::fs::{File, FileTimes};
    use std::path::Path;
    use std::time::SystemTime;

    /// Writes `contents` to `path` (parents made) and sets its modification time.
    pub(crate) fn write(path: &Path, contents: &[u8], mtime: SystemTime) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
        let file = File::options().write(true).open(path).unwrap();
        file.set_times(FileTimes::new().set_modified(mtime))
            .unwrap();
    }

    /// Removes `path` and everything under it, if it exists.
    pub(crate) fn remove_all(path: &Path) {
        if let Err(err) = std::fs::remove_dir_all(path) {
            assert_eq!(err.kind(), std::io::ErrorKind::NotFound, "{err}");
        }
    }
}

/// A xorshift64 generator: the same folder on every run.
pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// `len` bytes: random after `magic`, or text-like (compressible) when `magic` is empty.
    pub(crate) fn contents(&mut self, magic: &[u8], len: usize) -> Vec<u8> {
        const WORDS: [&str; 8] = [
            "agreement ",
            "the party ",
            "shall ",
            "indemnify ",
            "notwithstanding ",
            "clause ",
            "hereinafter ",
            "governing law\n",
        ];
        let mut bytes = Vec::with_capacity(len + 16);
        bytes.extend_from_slice(magic);
        while bytes.len() < len {
            let word = self.next();
            if magic.is_empty() {
                let index = usize::try_from(word % 8).unwrap();
                bytes.extend_from_slice(WORDS[index].as_bytes());
            } else {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        bytes.truncate(len);
        bytes
    }
}

/// `seconds` after the Unix epoch.
pub(crate) fn at_seconds(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

/// Checkpoint `n`'s id: the counter in the UUID's low bits.
pub(crate) fn checkpoint_id(n: u64) -> CheckpointId {
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

/// Imports `folder` as a matter under the matters root `root`, root checkpoint 1.
pub(crate) fn import(root: &Path, folder: &Path) -> Matter {
    let settings = Settings {
        matters_root: root.to_owned(),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::new(OsProbe),
    };
    let new = NewMatter {
        id: "01910000-0000-7000-8000-000000000001".parse().unwrap(),
        folder: folder.to_owned(),
        created_at: Timestamp::new(1_700_000_000_000).unwrap(),
        marker: DEFAULT_MARKER,
        checkpoint: checkpoint_id(1),
    };
    let cancel = AtomicBool::new(false);
    match Matter::import(settings, new, &[], &mut |_| {}, &cancel).unwrap() {
        Imported::Done { matter, .. } => *matter,
        Imported::Cancelled => unreachable!("no cancel flag was set"),
    }
}
