//! A preview of a source on this computer: a folder (read in place), a `SKILL.md` (its folder,
//! that skill alone) or a `.zip` (read whole, at most [`DOWNLOAD_MAX`] bytes, then unpacked).
//! Off the host thread.

use std::io::Read as _;
use std::path::Path;

use super::super::read::is_reparse;
use super::stage::{Preview, Tree, stage_tree};
use super::{DOWNLOAD_MAX, FromKind, Origin, SourceError};
use crate::skills::SKILL_MD;

/// Stages the source at `path` under `adding`. `NotFound` when nothing there is a skill,
/// `BadSource` for a link or anything that is not a folder, a `SKILL.md` or a `.zip`.
pub fn preview_local(adding: &Path, path: &Path, now_ms: i64) -> Result<Preview, SourceError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_gone| SourceError::NotFound)?;
    if meta.file_type().is_symlink() || is_reparse(&meta) {
        return Err(SourceError::BadSource);
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if meta.is_dir() {
        let origin = Origin::local(FromKind::Folder, path);
        return stage_tree(adding, Tree::Folder(path), origin, None, now_ms);
    }
    if !meta.is_file() {
        return Err(SourceError::BadSource);
    }
    if name.eq_ignore_ascii_case(SKILL_MD) {
        let folder = path.parent().ok_or(SourceError::NotFound)?;
        let origin = Origin::local(FromKind::Folder, folder);
        let preview = stage_tree(adding, Tree::Folder(folder), origin, None, now_ms)?;
        // The folder's own SKILL.md, never a skill found below it.
        if !preview.skills.iter().any(|skill| skill.folder.is_empty()) {
            super::stage::close_preview(adding, &preview.token);
            return Err(SourceError::NotFound);
        }
        let skills = preview
            .skills
            .into_iter()
            .filter(|skill| skill.folder.is_empty())
            .collect();
        return Ok(Preview { skills, ..preview });
    }
    let is_zip = Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"));
    if !is_zip {
        return Err(SourceError::BadSource);
    }
    if meta.len() > DOWNLOAD_MAX {
        return Err(SourceError::TooBig);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(DOWNLOAD_MAX + 1).read_to_end(&mut bytes))
        .map_err(|_io| SourceError::Io)?;
    if bytes.len() as u64 > DOWNLOAD_MAX {
        return Err(SourceError::TooBig);
    }
    let tree = Tree::Zip {
        bytes: &bytes,
        strip_top: false,
        path: None,
    };
    stage_tree(
        adding,
        tree,
        Origin::local(FromKind::Zip, path),
        None,
        now_ms,
    )
}
