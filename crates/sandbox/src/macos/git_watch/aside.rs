//! Setting one item aside: the move into its own set-aside folder with its record, or, once the
//! agent is gone and the move still fails, the rename in place; and the lawyer's notice.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Asks, Fs, Kind, Phase};
use crate::utc;
use crate::{Denial, DenialSink, GitItem};

/// What an item that could not leave the matter gets after its name, then `-<n>`.
const IN_PLACE: &str = "set-aside-by-clauseo";

/// The item's name in its set-aside folder: not `.git` or `HEAD`, so no git finds it there.
const ITEM: &str = "item";

/// The record beside it.
const RECORD: &str = "record.json";

/// The most numbered names tried for one item before giving up.
const NAMES_MAX: u32 = 1000;

/// Where items go, and what each set-aside is recorded and reported with.
pub(super) struct Aside {
    pub(super) matter: PathBuf,
    /// The git set-aside folder.
    pub(super) folder: PathBuf,
    pub(super) session: String,
    pub(super) notices: Arc<dyn DenialSink>,
    /// Set once a failed move made the runner end the session.
    pub(super) ended_by_us: bool,
    /// Items set aside so far, for the folder names.
    pub(super) count: u32,
}

impl Aside {
    /// Moves `path` (a `what` of kind `kind`) out of the matter. A move that fails while the
    /// agent may run asks for the session's end; once it is gone, the item is renamed in place.
    pub(super) fn put<F: Fs>(
        &mut self,
        fs: &F,
        path: &Path,
        what: GitItem,
        kind: Kind,
        phase: Phase,
    ) -> Asks {
        let folder = self.item_folder(fs);
        let moved = match &folder {
            Ok(folder) => fs.rename_new(path, &folder.join(ITEM)),
            Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
        };
        match moved {
            Ok(()) => {
                let to = folder.ok().map(|folder| folder.join(ITEM));
                self.done(fs, path, what, kind, to, "moved", None);
                Asks::default()
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Asks::default(),
            Err(error) if phase == Phase::Running => {
                tracing::error!(
                    path = %path.display(), %error,
                    "the git watch could not move an item out of the matter; ending the session"
                );
                Asks {
                    rescan: false,
                    end: true,
                }
            }
            Err(error) => {
                let to = rename_in_place(fs, path);
                let how = if to.is_some() {
                    "renamed_in_place"
                } else {
                    "not_moved"
                };
                self.done(fs, path, what, kind, to, how, Some(&error));
                Asks::default()
            }
        }
    }

    /// A new folder for one item: `<folder>/<UTC time>-<session>-<n>`.
    fn item_folder<F: Fs>(&mut self, fs: &F) -> io::Result<PathBuf> {
        fs.make_dirs(&self.folder)?;
        let stamp = utc::timestamp(fs.now());
        for _ in 0..NAMES_MAX {
            self.count += 1;
            let folder = self
                .folder
                .join(format!("{stamp}-{}-{}", self.session, self.count));
            match fs.make_dir(&folder) {
                Ok(()) => return Ok(folder),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other("no free set-aside folder name"))
    }

    /// Records a set-aside and tells the lawyer.
    fn done<F: Fs>(
        &mut self,
        fs: &F,
        path: &Path,
        what: GitItem,
        kind: Kind,
        to: Option<PathBuf>,
        how: &str,
        error: Option<&io::Error>,
    ) {
        let rel = path.strip_prefix(&self.matter).unwrap_or(path).to_owned();
        let record = serde_json::json!({
            "original": path,
            "matter": self.matter,
            "path": rel,
            "what": what,
            "kind": format!("{kind:?}").to_lowercase(),
            "time": utc::rfc3339(fs.now()),
            "session": self.session,
            "how": how,
            "moved_to": to,
            "session_ended": self.ended_by_us,
            "error": error.map(ToString::to_string),
        });
        let folder = match &to {
            Some(to) if how == "moved" => to.parent().map(Path::to_owned),
            _ => self.item_folder(fs).ok(),
        };
        let written = match (folder, serde_json::to_vec_pretty(&record)) {
            (Some(folder), Ok(bytes)) => fs.write_new(&folder.join(RECORD), &bytes),
            (None, _) => Err(io::Error::other("no set-aside folder")),
            (_, Err(error)) => Err(io::Error::other(error)),
        };
        if let Err(error) = written {
            tracing::error!(path = %path.display(), %error, "the git watch could not record");
        }
        tracing::warn!(
            path = %path.display(), ?what, how, to = ?to,
            "the git watch set aside what an agent left in the matter"
        );
        self.notices.denied(Denial::GitSetAside {
            path: rel,
            what,
            moved_to: to,
            session_ended: self.ended_by_us,
        });
    }
}

/// Renames `path` in place to the first free `<name>-set-aside-by-clauseo-<n>`.
fn rename_in_place<F: Fs>(fs: &F, path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let name = path.file_name()?.to_string_lossy();
    for n in 1..=NAMES_MAX {
        let to = dir.join(format!("{name}-{IN_PLACE}-{n}"));
        match fs.rename_new(path, &to) {
            Ok(()) => return Some(to),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                tracing::error!(
                    path = %path.display(), %error,
                    "the git watch could not rename an item in place"
                );
                return None;
            }
        }
    }
    None
}
