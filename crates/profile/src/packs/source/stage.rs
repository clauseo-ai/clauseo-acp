//! A preview's staging folder, `<data>/skill-packs.adding/<token>/`, from the preview until the
//! lawyer picks (or [`STAGE_TTL_MS`] passes, or Clauseo starts again):
//!
//! - `made`: when it was made, written first, so a sweep by another preview leaves a folder
//!   still unpacking alone.
//! - `stage.json`: when it was made, where it came from ([`Origin`]), where its tree's top is
//!   (`tree/...` inside the folder for an archive, or the lawyer's own folder, read in place),
//!   and the skills the preview showed ([`Shown`]): the only ones `install_picked` installs.
//! - `tree/`: the archive, unpacked ([`super::unzip`]).
//!
//! A token is 32 lowercase hex characters, so it is never a pack id (at most 30 bytes) and
//! never meets `added::install`'s own `<id>` and `<id>.old` folders beside it. Off the host
//! thread.

use std::collections::hash_map::RandomState;
use std::fmt::Write as _;
use std::hash::{BuildHasher as _, Hasher as _};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clauseo_protocol::{Date, PackId};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::super::{Pack, Refusal, added};
use super::discover::{Found, discover};
use super::{FromKind, Origin, SourceError, unzip};
use crate::write;

/// The folder beside the packs folder that holds previews and installs in progress.
pub const ADDING_DIR: &str = "skill-packs.adding";

/// How long a preview lasts: 30 minutes.
pub const STAGE_TTL_MS: i64 = 30 * 60 * 1000;

/// The file in a preview's folder that describes it.
const STAGE_FILE: &str = "stage.json";

/// The most bytes of a `stage.json` read back: room for the names and folders of every skill a
/// large repository shows.
const STAGE_MAX: u64 = 1024 * 1024;

/// The file in a preview's folder written first, holding when it was made.
const MADE_FILE: &str = "made";

/// The folder in a preview's folder an archive is unpacked into.
const TREE_DIR: &str = "tree";

/// A preview: its token, where it came from, the skills found and when it was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// The token `add_skills` names it by.
    pub token: String,
    /// Where it came from.
    pub origin: Origin,
    /// The skills found, in the order found.
    pub skills: Vec<Found>,
    /// When it was made (the host clock's wall time, Unix milliseconds).
    pub made_ms: i64,
}

/// What a preview holds.
#[derive(Debug, Clone, Copy)]
pub enum Tree<'a> {
    /// A folder on this computer (or the folder of a `SKILL.md`), read in place.
    Folder(&'a Path),
    /// An archive: unpacked; with `strip_top` its one top folder is the top (GitHub's archive
    /// holds `<repo>-<ref>/`), then narrowed to `path`.
    Zip {
        /// The archive's bytes.
        bytes: &'a [u8],
        /// Whether the archive's single top folder is skipped.
        strip_top: bool,
        /// The folder inside to look in.
        path: Option<&'a str>,
    },
}

/// Stages `tree` under `adding` and finds its skills (only `asked`, when given). Nothing stays
/// behind on an error, including `NotFound` (no skill, or none by that name).
pub fn stage_tree(
    adding: &Path,
    tree: Tree<'_>,
    origin: Origin,
    asked: Option<&str>,
    now_ms: i64,
) -> Result<Preview, SourceError> {
    write::create_dir_all(adding).map_err(|_io| SourceError::Io)?;
    let token = new_token(now_ms);
    let folder = adding.join(&token);
    write::create_dir(&folder).map_err(|_io| SourceError::Io)?;
    let made = now_ms.to_string();
    let result = write::write_new(&folder.join(MADE_FILE), made.as_bytes())
        .map_err(|_io| SourceError::Io)
        .and_then(|()| fill(&folder, tree, &origin, asked, now_ms));
    match result {
        Ok(skills) => Ok(Preview {
            token,
            origin,
            skills,
            made_ms: now_ms,
        }),
        Err(error) => {
            write::remove_dir_all(&folder);
            Err(error)
        }
    }
}

/// Unpacks or points at the tree, finds the skills and writes `stage.json`.
fn fill(
    folder: &Path,
    tree: Tree<'_>,
    origin: &Origin,
    asked: Option<&str>,
    now_ms: i64,
) -> Result<Vec<Found>, SourceError> {
    let top = match tree {
        Tree::Folder(path) => path.to_path_buf(),
        Tree::Zip {
            bytes,
            strip_top,
            path,
        } => {
            let unpacked = folder.join(TREE_DIR);
            unzip(bytes, &unpacked)?;
            let mut top = unpacked;
            if strip_top {
                top = only_folder(&top).ok_or(SourceError::NotFound)?;
            }
            for part in path
                .unwrap_or_default()
                .split('/')
                .filter(|p| !p.is_empty())
            {
                top.push(part);
            }
            top
        }
    };
    let real = std::fs::symlink_metadata(&top)
        .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink());
    if !real {
        return Err(SourceError::NotFound);
    }
    let skills = discover(&top, asked)?;
    let shown: Vec<Value> = skills
        .iter()
        .filter(|skill| skill.problem.is_none())
        .map(|skill| json!({"name": skill.name, "folder": skill.folder}))
        .collect();
    let stage = json!({
        "made_ms": now_ms,
        "kind": origin.kind.as_str(),
        "text": origin.text,
        "source_url": origin.source_url,
        "top": top.to_string_lossy(),
        "shown": shown,
    });
    let bytes = serde_json::to_vec(&stage).map_err(|_bad| SourceError::Io)?;
    write::write_new(&folder.join(STAGE_FILE), &bytes).map_err(|_io| SourceError::Io)?;
    Ok(skills)
}

/// The one folder in `dir`, when it holds exactly one entry and that is a folder.
fn only_folder(dir: &Path) -> Option<PathBuf> {
    let mut entries = std::fs::read_dir(dir).ok()?.flatten();
    let first = entries.next()?;
    if entries.next().is_some() {
        return None;
    }
    let meta = std::fs::symlink_metadata(first.path()).ok()?;
    (meta.is_dir() && !meta.file_type().is_symlink()).then(|| first.path())
}

/// A preview found again by its token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    /// Its folder under `skill-packs.adding`.
    pub folder: PathBuf,
    /// Its tree's top.
    pub top: PathBuf,
    /// Where it came from.
    pub origin: Origin,
    /// When it was made.
    pub made_ms: i64,
    /// The skills the preview showed, the only ones that can be installed from it.
    pub shown: Vec<Shown>,
}

/// One skill a preview showed: a [`Found`] that could be added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// Its listed name ([`Found::name`]).
    pub name: String,
    /// Its folder, relative to the tree's top, `/`-separated; empty for the top itself.
    pub folder: String,
}

impl Staged {
    /// The skill the preview showed as `name`, if it did.
    pub fn shown(&self, name: &str) -> Option<&Shown> {
        self.shown.iter().find(|skill| skill.name == name)
    }
}

/// The preview `token` under `adding`: `Expired` when there is none, or it is past
/// [`STAGE_TTL_MS`] at `now_ms` (its folder is then removed).
pub fn open_preview(adding: &Path, token: &str, now_ms: i64) -> Result<Staged, SourceError> {
    if !is_token(token) {
        return Err(SourceError::Expired);
    }
    let folder = adding.join(token);
    let staged = read_stage(&folder).ok_or(SourceError::Expired)?;
    if now_ms.saturating_sub(staged.made_ms) > STAGE_TTL_MS || now_ms < staged.made_ms {
        write::remove_dir_all(&folder);
        return Err(SourceError::Expired);
    }
    Ok(staged)
}

/// The `stage.json` of the preview folder `folder`.
fn read_stage(folder: &Path) -> Option<Staged> {
    let path = folder.join(STAGE_FILE);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .ok()?
        .take(STAGE_MAX)
        .read_to_end(&mut bytes)
        .ok()?;
    let stage: Value = serde_json::from_slice(&bytes).ok()?;
    let text = |key: &str| stage.get(key).and_then(Value::as_str).map(str::to_owned);
    let shown = stage
        .get("shown")
        .and_then(Value::as_array)
        .map(|shown| {
            shown
                .iter()
                .filter_map(|skill| {
                    Some(Shown {
                        name: skill.get("name")?.as_str()?.to_owned(),
                        folder: skill.get("folder")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Staged {
        folder: folder.to_path_buf(),
        top: PathBuf::from(text("top")?),
        origin: Origin {
            kind: FromKind::parse(&text("kind")?)?,
            text: text("text")?,
            source_url: text("source_url"),
        },
        made_ms: stage.get("made_ms")?.as_i64()?,
        shown,
    })
}

/// When the preview folder `folder` was made: its `stage.json`, else its `made` file (a
/// preview still being filled).
fn made_ms(folder: &Path) -> Option<i64> {
    if let Some(staged) = read_stage(folder) {
        return Some(staged.made_ms);
    }
    let path = folder.join(MADE_FILE);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > 32 {
        return None;
    }
    std::fs::read_to_string(&path).ok()?.trim().parse().ok()
}

/// Ends the preview `token` (after its install, or when the lawyer cancels): its folder goes.
pub fn close_preview(adding: &Path, token: &str) {
    if is_token(token) {
        write::remove_dir_all(&adding.join(token));
    }
}

/// Removes the previews under `adding`: every one when `now_ms` is `None` (Clauseo starting),
/// else those past [`STAGE_TTL_MS`] or unreadable (one still being filled is read by its
/// `made` file). Anything not named as a token is left.
pub fn sweep(adding: &Path, now_ms: Option<i64>) {
    let Ok(entries) = std::fs::read_dir(adding) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_token(&name) {
            continue;
        }
        let keep = now_ms.is_some_and(|now| {
            made_ms(&entry.path()).is_some_and(|made| made <= now && now - made <= STAGE_TTL_MS)
        });
        if !keep {
            write::remove_dir_all(&entry.path());
        }
    }
}

/// One skill installed from a preview.
#[derive(Debug, Clone)]
pub struct Installed {
    /// The pack's id.
    pub id: PackId,
    /// The pack as written (its hash is what `skills.enabled` records).
    pub pack: Pack,
}

/// Installs the skill the preview showed as `pick` (a [`Shown::name`]) from `staged` as a pack
/// under `packs_root`, through `adding`, recording where it came from; a name it did not show
/// (hidden, narrowed away, refused, or not there) is `NoSkillMd`, nothing written. `admit`
/// says whether the firm admits the pack id it would take (`Ok(None)` when it does not,
/// nothing written). Scripts come along when `allow_scripts`. Adding a name added before
/// replaces it.
pub fn install_picked(
    staged: &Staged,
    pick: &str,
    (packs_root, adding): (&Path, &Path),
    (today, allow_scripts): (Date, bool),
    admit: &dyn Fn(&str) -> bool,
) -> Result<Option<Installed>, added::AddError> {
    let invalid = added::AddError::Invalid;
    let skill = staged.shown(pick).ok_or(added::AddError::NoSkillMd)?;
    let folder = if skill.folder.is_empty() {
        staged.top.clone()
    } else {
        shown_folder(&staged.top, &skill.folder).ok_or(added::AddError::NoSkillMd)?
    };
    let source = added::read_source(&folder)?;
    let id = added::free_id(packs_root, &source.slug)?;
    let pack_id = PackId::new(&id).map_err(|_bad| invalid(Refusal::BadId))?;
    if !admit(&id) {
        return Ok(None);
    }
    let record = added::Record::of(&staged.origin);
    let (files, pack) = added::build_from(&source, &id, today, allow_scripts, &record)?;
    added::install(packs_root, adding, &id, &files)?;
    Ok(Some(Installed { id: pack_id, pack }))
}

/// `top` joined with a shown skill's `folder` (`stage.json`'s record of a folder the walk found):
/// each part a name (not empty, `.` or `..`, no `\` or `:`), staying under `top`. Unlike
/// `discover::inside`, no archive-name rule (an 8.3 short name such as `x~1`, a trailing dot):
/// the lawyer's own picked folder may use them and must add as it previewed, and an archive's
/// names passed `unzip`'s rules already.
fn shown_folder(top: &Path, folder: &str) -> Option<PathBuf> {
    let mut out = top.to_path_buf();
    for part in folder.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':']) {
            return None;
        }
        out.push(part);
    }
    let rest = out.strip_prefix(top).ok()?;
    rest.components()
        .all(|part| matches!(part, std::path::Component::Normal(_)))
        .then_some(out)
}

/// Whether `name` is a token: 32 lowercase hex characters.
fn is_token(name: &str) -> bool {
    name.len() == 32
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A fresh token: the sha256 of std's random hash keys, a counter, `now_ms` and the process
/// id, cut to 32 hex characters. It names a folder only; nothing is unlocked by knowing it.
fn new_token(now_ms: i64) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    let mut digest = Sha256::new();
    digest.update(hasher.finish().to_le_bytes());
    digest.update(now_ms.to_le_bytes());
    digest.update(std::process::id().to_le_bytes());
    let mut token = String::with_capacity(32);
    for byte in digest.finalize().iter().take(16) {
        let _infallible = write!(token, "{byte:02x}");
    }
    token
}
