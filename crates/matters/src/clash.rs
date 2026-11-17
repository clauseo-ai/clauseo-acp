//! A file and a folder that meet where a restore writes: a folder where the file goes, or a file
//! where one of its folders goes. The entry in the way is named as the folder spells it, so the
//! lawyer reads the name they see: on a case- or normalisation-insensitive file system (APFS by
//! default, NTFS) the version's `Memo.docx` finds the folder `memo.docx`.

use std::ffi::OsStr;
use std::fs::Metadata;
use std::io;
use std::path::Path;

use clauseo_protocol::MatterPath;

use crate::Error;

#[cfg(test)]
mod tests;

/// The most entries [`spelled`] reads from one folder.
const MAX_LISTED: usize = 20_000;

/// The entry in the folder of the other kind that `path` meets, by its matter path as the folder
/// spells it: a folder where the file goes (`path` itself), or a file, pipe, socket or device
/// where one of its folders goes (that folder's path). `None` when it meets none; a link, or
/// nothing, on the way is no clash ([`crate::writeback::writable`] refuses a link).
pub(crate) fn clash(folder: &Path, path: &MatterPath) -> Result<Option<MatterPath>, Error> {
    let mut at = folder.to_owned();
    let mut seen: Vec<String> = Vec::new();
    let mut parts = path.as_str().split('/').peekable();
    while let Some(part) = parts.next() {
        let metadata = match std::fs::symlink_metadata(at.join(part)) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(Error::io("stat", at.join(part), err)),
        };
        let kind = metadata.file_type();
        if kind.is_symlink() {
            return Ok(None);
        }
        let name = spelled(&at, part, &metadata);
        at.push(&name);
        seen.push(name);
        let last = parts.peek().is_none();
        // a folder where the file goes, or anything but a folder (a file, a pipe, a socket)
        // where one of its folders goes
        if (last && kind.is_dir()) || (!last && !kind.is_dir()) {
            let in_way =
                MatterPath::new(&seen.join("/")).map_err(|err| Error::history("clash", err))?;
            return Ok(Some(in_way));
        }
    }
    Ok(None)
}

/// The name `dir` holds the entry `name` finds under (`metadata` its own): `name` when an
/// entry is called exactly that, else the entry spelled otherwise (another case or Unicode
/// normalisation) that is the same one; `name` when the folder can't be read or none is found.
fn spelled(dir: &Path, name: &str, metadata: &Metadata) -> String {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return name.to_owned();
    };
    let mut other = None;
    for entry in entries.flatten().take(MAX_LISTED) {
        let found = entry.file_name();
        if found == OsStr::new(name) {
            return name.to_owned();
        }
        let at = dir.join(&found);
        if other.is_none()
            && std::fs::symlink_metadata(&at)
                .is_ok_and(|its| same_entry(&at, &its, &dir.join(name), metadata))
        {
            other = found.into_string().ok();
        }
    }
    other.unwrap_or_else(|| name.to_owned())
}

/// Whether the entries found at `a` and `b` are one file: same device and inode on Unix;
/// elsewhere, where std gives no file id, paths equal but for case.
pub(crate) fn same_entry(a: &Path, a_meta: &Metadata, b: &Path, b_meta: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = (a, b);
        a_meta.dev() == b_meta.dev() && a_meta.ino() == b_meta.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (a_meta, b_meta);
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
}
