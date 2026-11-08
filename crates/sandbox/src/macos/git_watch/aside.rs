//! Setting one item aside once the session has ended: the move into its own set-aside folder
//! with its record and its `HEAD` and `config` renamed, or, when the move fails, neutralising
//! it in place; and the lawyer's notice.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Fs, Kind, is_name};
use crate::utc;
use crate::{Denial, DenialSink, GitItem, GitOutcome};

/// What a renamed name gets after it, then a random number.
const RENAMED: &str = "set-aside-by-clauseo";

/// The item's name in its set-aside folder: not `.git` or `HEAD`, so no git finds it there.
const ITEM: &str = "item";

/// The record beside it.
const RECORD: &str = "record.json";

/// The most random names tried for one rename before giving up.
const NAMES_MAX: u32 = 16;

/// The names in a git folder that make it one, renamed in a set-aside or neutralised item.
const GIT_NAMES: [&str; 2] = ["HEAD", "config"];

/// An item that could neither move nor be neutralised: the matter's next launch is refused
/// while it is there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stuck {
    pub(crate) path: PathBuf,
    pub(crate) reason: String,
}

/// Where items go, and what each set-aside is recorded and reported with.
pub(crate) struct Aside {
    pub(crate) matter: PathBuf,
    /// The git set-aside folder on the matter's volume.
    pub(crate) folder: PathBuf,
    pub(crate) session: String,
    pub(crate) notices: Arc<dyn DenialSink>,
    /// Set once a find made the runner end the session.
    pub(crate) ended_by_us: bool,
    /// The items that could neither move nor be neutralised.
    pub(crate) stuck: Vec<Stuck>,
}

impl Aside {
    pub(crate) fn new(
        matter: PathBuf,
        folder: PathBuf,
        session: String,
        notices: Arc<dyn DenialSink>,
    ) -> Aside {
        Aside {
            matter,
            folder,
            session,
            notices,
            ended_by_us: false,
            stuck: Vec::new(),
        }
    }

    /// Moves `path` (a `what` of kind `kind`) out of the matter; when that fails, neutralises
    /// it in place; when that fails too, keeps it as [`Stuck`].
    pub(super) fn put<F: Fs>(&mut self, fs: &F, path: &Path, what: GitItem, kind: Kind) {
        let target = (kind == Kind::Link)
            .then(|| fs.read_link(path).ok())
            .flatten();
        let folder = self.item_folder(fs);
        let moved = match &folder {
            Ok(folder) => fs.rename_new(path, &folder.join(ITEM)),
            Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
        };
        let (outcome, error) = match (moved, folder) {
            (Ok(()), Ok(folder)) => {
                let item = folder.join(ITEM);
                if kind == Kind::Dir
                    && let Err(error) = rename_inside(fs, &item)
                {
                    tracing::error!(%error, item = %item.display(), "a set-aside HEAD stays");
                }
                (GitOutcome::Moved(item), None)
            }
            (Err(error), _) if error.kind() == io::ErrorKind::NotFound => return,
            (Err(error), _) | (Ok(()), Err(error)) => {
                tracing::error!(
                    path = %path.display(), %error,
                    "the git watch could not move an item out of the matter; neutralising it"
                );
                match in_place(fs, path, kind, target.clone()) {
                    Ok(outcome) => (outcome, Some(error)),
                    Err(again) => {
                        tracing::error!(path = %path.display(), %again, "nor in place");
                        let reason = format!("{error}; in place: {again}");
                        self.stuck.push(Stuck {
                            path: path.to_owned(),
                            reason: reason.clone(),
                        });
                        (GitOutcome::Stuck, Some(io::Error::other(reason)))
                    }
                }
            }
        };
        self.done(
            fs,
            path,
            what,
            kind,
            outcome,
            target.as_deref(),
            error.as_ref(),
        );
    }

    /// A new folder for one item: `<folder>/<UTC time>-<session>-<random>`.
    fn item_folder<F: Fs>(&mut self, fs: &F) -> io::Result<PathBuf> {
        fs.make_dirs(&self.folder)?;
        let stamp = utc::timestamp(fs.now());
        for _ in 0..NAMES_MAX {
            let folder = self
                .folder
                .join(format!("{stamp}-{}-{:016x}", self.session, fs.random()));
            match fs.make_dir(&folder) {
                Ok(()) => return Ok(folder),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other("no free set-aside folder name"))
    }

    /// Records a set-aside and tells the lawyer.
    #[expect(clippy::too_many_arguments, reason = "one record's fields")]
    fn done<F: Fs>(
        &mut self,
        fs: &F,
        path: &Path,
        what: GitItem,
        kind: Kind,
        outcome: GitOutcome,
        target: Option<&Path>,
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
            "outcome": outcome,
            "link_target": target,
            "session_ended": self.ended_by_us,
            "error": error.map(ToString::to_string),
        });
        let folder = match &outcome {
            GitOutcome::Moved(to) => to.parent().map(Path::to_owned),
            GitOutcome::RenamedInPlace(_) | GitOutcome::LinkRemoved { .. } | GitOutcome::Stuck => {
                self.item_folder(fs).ok()
            }
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
            path = %path.display(), ?what, ?outcome,
            "the git watch set aside what an agent left in the matter"
        );
        self.notices.denied(Denial::GitSetAside {
            path: rel,
            what,
            outcome,
            session_ended: self.ended_by_us,
        });
    }
}

/// Neutralises the item at `path` where it is: a link is removed (its `target` recorded); a
/// folder's `HEAD` and `config` are renamed, and a `.git` or another folder that isn't a bare
/// repository's is renamed too; a file is renamed.
fn in_place<F: Fs>(
    fs: &F,
    path: &Path,
    kind: Kind,
    target: Option<PathBuf>,
) -> io::Result<GitOutcome> {
    match kind {
        Kind::Link => {
            fs.remove_link(path)?;
            Ok(GitOutcome::LinkRemoved {
                target: target.unwrap_or_default(),
            })
        }
        Kind::Dir => {
            let head = rename_inside(fs, path)?;
            if path.file_name().is_some_and(|name| is_name(name, ".git")) || head.is_none() {
                rename_random(fs, path).map(GitOutcome::RenamedInPlace)
            } else {
                Ok(GitOutcome::RenamedInPlace(head.unwrap_or_default()))
            }
        }
        Kind::File | Kind::Other => rename_random(fs, path).map(GitOutcome::RenamedInPlace),
    }
}

/// Renames the `HEAD` and `config` in folder `dir`, links included; returns the `HEAD`'s new
/// path, `None` without one.
fn rename_inside<F: Fs>(fs: &F, dir: &Path) -> io::Result<Option<PathBuf>> {
    let mut head = None;
    for (name, kind) in fs.list(dir)? {
        let Some(want) = GIT_NAMES.iter().find(|want| is_name(&name, want)) else {
            continue;
        };
        if matches!(kind, Kind::File | Kind::Link | Kind::Dir) {
            let to = rename_random(fs, &dir.join(&name))?;
            if *want == "HEAD" {
                head = Some(to);
            }
        }
    }
    Ok(head)
}

/// Renames `path` in place to `<name>-set-aside-by-clauseo-<random>`.
fn rename_random<F: Fs>(fs: &F, path: &Path) -> io::Result<PathBuf> {
    let dir = path.parent().ok_or_else(|| io::Error::other("no folder"))?;
    let name = path
        .file_name()
        .map_or_else(Default::default, OsStr::to_string_lossy);
    for _ in 0..NAMES_MAX {
        let to = dir.join(format!("{name}-{RENAMED}-{:016x}", fs.random()));
        match fs.rename_new(path, &to) {
            Ok(()) => return Ok(to),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("no free name"))
}
