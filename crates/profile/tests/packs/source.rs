//! The skill sources (`packs::source`, decision record 0040, "Adding from a link"): discovery,
//! the archive's refusals, the link and name table, and the preview's staging and install. Each
//! test works in a folder of its own under `CARGO_TARGET_TMPDIR`, removed when it ends; files
//! change through `support` alone.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::support;

#[path = "source/discover.rs"]
mod discover;
#[path = "source/remote.rs"]
mod remote;
#[path = "source/stage.rs"]
mod stage;
#[path = "source/unzip.rs"]
mod unzip;

/// A folder of its own, removed on drop.
pub(super) struct Scratch(PathBuf);

impl Scratch {
    pub(super) fn new(test: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "clauseo-skill-source-{test}-{}",
            std::process::id()
        ));
        support::fresh_dir(&dir).unwrap();
        Self(dir)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _gone = support::remove_dir_all(&self.0);
    }
}

/// Writes `bytes` at `top/rel`, its folders made.
pub(super) fn put(top: &Path, rel: &str, bytes: &[u8]) {
    support::write(&top.join(rel), bytes).unwrap();
}

/// A symlink at `link` to `target`.
#[cfg(unix)]
pub(super) fn symlink(target: &Path, link: &Path) {
    support::symlink(target, link).unwrap();
}

/// Sets `path`'s Unix mode.
#[cfg(unix)]
pub(super) fn set_mode(path: &Path, mode: u32) {
    support::set_mode(path, mode).unwrap();
}

/// A `SKILL.md` named `name`.
pub(super) fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Does {name} things.\n---\n# {name}\n\nBody.\n")
}

/// Writes a skill named `name` in the folder `rel` under `top`.
pub(super) fn put_skill(top: &Path, rel: &str, name: &str) {
    let at = if rel.is_empty() {
        "SKILL.md".to_owned()
    } else {
        format!("{rel}/SKILL.md")
    };
    put(top, &at, skill_md(name).as_bytes());
}

/// A zip of `entries` (name, bytes), deflated.
pub(super) fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        if let Some(dir) = name.strip_suffix('/') {
            zip.add_directory(dir, options).unwrap();
        } else {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
    }
    zip.finish().unwrap().into_inner()
}
