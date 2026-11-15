//! The fence between matters: what of every other matter the host knows (registered or
//! forgotten) a session's agent is kept out of, and what a per-conversation read invite opens
//! (the header's `read_matters`, `MatterReadAllowed`). Made at the launch: the launch paths
//! name the other matters ([`MatterFence::asked`]); the launch prep resolves their folders and
//! lists what nesting needs ([`MatterFence::resolve`]).
//!
//! For each other matter its fence paths ([`Other::paths`]) are its folder whole, each spelling
//! (where it leads, resolved like Undo's paths through `network_paths::touchable`, then as stored
//! when that differs; a folder on a share as stored alone, never touched), unless the folder
//! holds a kept place: the session's matter folder or `<data>` (each spelling), or, for a matter
//! the session is not invited into, a matter it is. Then they are the siblings along the way
//! down to each kept place, listed now (as `codex_fence::down_to` does for `<data>`), so a deny
//! never covers the session's own matter. A matter inside the session's matter is fenced whole;
//! the session's own matter never is. A folder on the way that can't be listed (or a matter on
//! a share that holds a kept place) leaves that matter unfenced, and so does a matter whose
//! paths would take the launch past [`MAX_PATHS`] (Claude's settings would outgrow the file
//! Claude Code reads): a deny of the whole folder would cover the session's own, so the lawyer
//! is told instead ([`MatterFence::unfenced`], `NoticeKind::MattersUnfenced`).
//!
//! A matter inside the session's own matter needs one more thing on macOS and Linux: Claude's
//! sandbox re-opens the own matter for reading, and its `allowRead` beats its `denyRead`, so it
//! re-opens the own matter's entries one by one down to that matter instead
//! ([`MatterFence::own_open`]). Windows has no OS fence: there a command in the own matter that
//! never names the nested one (`rg x .`) reads it.
//!
//! Who uses which: Claude's `Read` deny rules and Codex's profile name the fence paths of the
//! matters not invited, Claude's `Edit` deny rules and its Windows shell rules those of every
//! other matter (an invite is read-only, and on Windows Claude reads an invited matter through
//! its file tools only), and Claude's `Read` allow rules the folders of the invited ones
//! (`data_fence::claude_deny_rules`, `claude_allow_rules`, `codex_fence::rules`,
//! `data_fence::shell_deny::matters`). The residual, as for `<data>`: what is made in a parent
//! matter after the launch is in no rule until the next launch.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf, Prefix};

use clauseo_protocol::MatterId;

use crate::network_paths::resolve::Disk;
use crate::network_paths::{FOLD_CASE, NetEnv, fold_name, touchable};

/// Another matter, as a launch fences it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Other {
    /// Its id.
    pub(crate) id: MatterId,
    /// Its name at the launch, for the lawyer's line when its shell rules did not fit.
    pub(crate) name: String,
    /// Its folder as stored.
    pub(crate) folder: PathBuf,
    /// Whether the lawyer let this conversation read its files.
    pub(crate) invited: bool,
    /// Its folder's spellings ([`MatterFence::resolve`]); empty before.
    pub(crate) spellings: Vec<PathBuf>,
    /// What of it the session is kept out of (the module doc); empty before the resolve.
    pub(crate) paths: Vec<PathBuf>,
}

impl Other {
    /// `id`'s matter at `folder`, not yet resolved.
    pub(crate) fn new(id: MatterId, name: String, folder: PathBuf, invited: bool) -> Self {
        Self {
            id,
            name,
            folder,
            invited,
            spellings: Vec::new(),
            paths: Vec::new(),
        }
    }
}

/// What a launch's fence was made of, to tell when it is out of date
/// (`actor::turn::outside::fence`): a hash of every matter's id and folder the host knows, and
/// one of the thread's invites. In memory only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FenceKey {
    /// The matters' ids and folders, hashed.
    pub(crate) matters: u64,
    /// The thread's invites, hashed.
    pub(crate) invited: u64,
}

/// Every other matter a launch fences (the module doc).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MatterFence {
    /// The other matters, sorted by id.
    pub(crate) others: Vec<Other>,
    /// What the launch made them of.
    pub(crate) key: FenceKey,
    /// The session's own matter folder, each spelling ([`Self::resolve`]).
    pub(crate) own: Vec<PathBuf>,
    /// When another matter the session is not invited into lies inside its own matter: what
    /// of the own matter Claude's sandbox re-opens for reading instead of the whole folder, its
    /// entries down to each such matter ([`down_to`]), so the fence holds for a command that
    /// never names the path (`rg x .`). Left out, as the agent controls them: links (re-opened,
    /// a link would re-open where it leads), and names that hold `*` or `?` or no letter or
    /// digit (the sandbox writes each entry as a pattern, [`reopen_safe`]). `None` when no such
    /// matter lies inside it; empty when the list could not be made safely
    /// ([`Self::own_unlisted`]).
    pub(crate) own_open: Option<Vec<PathBuf>>,
    /// Whether [`Self::own_open`] was needed but could not be made safely: a folder on the way
    /// could not be listed, is a link or on a network share, or the entries pass
    /// [`MAX_PATHS`]. Then the sandbox stays on and re-opens none of the own matter's entries
    /// (less access, never none of the sandbox: nothing in the matter may turn it off), and the
    /// lawyer is told (`NoticeKind::ShellLimited`).
    pub(crate) own_unlisted: bool,
    /// Every other matter's folder (each spelling, invited or not) inside the own matter:
    /// Claude's sandbox denies writes there, Codex's profile gives an invited one `read`.
    pub(crate) inside_own: Vec<(PathBuf, bool)>,
    /// The other matters left unfenced ([`Self::resolve`]): one inside a folder that could not
    /// be listed or on a network share, or past the launch's [`MAX_PATHS`]. The lawyer is told
    /// (`NoticeKind::MattersUnfenced`).
    pub(crate) unfenced: Vec<MatterId>,
}

/// The most fence paths a launch names, all matters together, and the most entries of the own
/// matter Claude's sandbox re-opens: past them, Claude's settings would near the 2 MiB file
/// Claude Code reads (`agent::launch_settings`), and the launch would fail.
pub(crate) const MAX_PATHS: usize = 2000;

impl MatterFence {
    /// No other matter (a unit test's keys).
    #[cfg(test)]
    pub(crate) const NONE: Self = Self {
        others: Vec::new(),
        key: FenceKey {
            matters: 0,
            invited: 0,
        },
        own: Vec::new(),
        own_open: None,
        own_unlisted: false,
        inside_own: Vec::new(),
        unfenced: Vec::new(),
    };

    /// The fence before the launch prep resolves it: `others`, sorted by id, so a launch's rules
    /// and the matters its shell rules keep (`agent::loads::profile`) do not depend on a map's
    /// order.
    pub(crate) fn asked(mut others: Vec<Other>) -> Self {
        others.sort_by_key(|other| other.id);
        Self {
            others,
            ..Self::default()
        }
    }

    /// Fills each matter's spellings and fence paths, the session's matter folder spelled each
    /// of `own` ways and `<data>` each of `data` ways. Reads the file system: off the actor.
    pub(crate) fn resolve(mut self, own: &[&Path], data: &[&Path]) -> Self {
        let net = NetEnv::current();
        let disk = Disk::resolved(&net);
        self.own = own.iter().map(|place| place.to_path_buf()).collect();
        for other in &mut self.others {
            other.spellings = spellings(&other.folder, &disk);
        }
        let invited: Vec<PathBuf> = self
            .others
            .iter()
            .filter(|other| other.invited)
            .flat_map(|other| other.spellings.iter().cloned())
            .collect();
        for other in &mut self.others {
            let mut keep: Vec<&Path> = own.iter().chain(data).copied().collect();
            if !other.invited {
                keep.extend(invited.iter().map(PathBuf::as_path));
            }
            let mut paths = Vec::new();
            let mut whole = true;
            for spelling in &other.spellings {
                match fence_paths(spelling, &keep, &net) {
                    Ok(found) => paths.extend(found),
                    Err(()) => whole = false,
                }
            }
            let mut seen = std::collections::HashSet::new();
            paths.retain(|path| seen.insert(path.clone()));
            if !whole {
                self.unfenced.push(other.id);
            }
            other.paths = paths;
        }
        self.spend_budget();
        if !self.unfenced.is_empty() {
            let unfenced = self.unfenced.len();
            tracing::warn!(
                unfenced,
                "other matters could not be fenced; the lawyer is told"
            );
        }
        self.split_own(&net);
        self
    }

    /// Keeps the launch's fence paths within [`MAX_PATHS`]: the matters with the fewest paths
    /// first (a plain sibling has one or two), so one parent with thousands of entries can't push
    /// every other matter out; a matter that does not fit loses its paths and joins
    /// [`Self::unfenced`]. Ties go by id.
    fn spend_budget(&mut self) {
        let mut order: Vec<usize> = (0..self.others.len()).collect();
        order.sort_by_key(|at| {
            self.others
                .get(*at)
                .map(|other| (other.paths.len(), other.id))
        });
        let mut total = 0_usize;
        for at in order {
            let Some(other) = self.others.get_mut(at) else {
                continue;
            };
            if total + other.paths.len() > MAX_PATHS {
                other.paths.clear();
                if !self.unfenced.contains(&other.id) {
                    self.unfenced.push(other.id);
                }
            }
            total += other.paths.len();
        }
        self.unfenced.sort();
    }

    /// Fills [`Self::inside_own`], [`Self::own_open`] and [`Self::own_unlisted`].
    fn split_own(&mut self, net: &NetEnv) {
        let inside = |path: &Path, own: &Path| {
            let (path, own) = (key(path), key(own));
            path.len() > own.len() && path.starts_with(&own)
        };
        for other in &self.others {
            for spelling in &other.spellings {
                if self.own.iter().any(|own| inside(spelling, own)) {
                    self.inside_own.push((spelling.clone(), other.invited));
                }
            }
        }
        let fenced: Vec<&Path> = self
            .inside_own
            .iter()
            .filter(|(_, invited)| !invited)
            .map(|(path, _)| path.as_path())
            .collect();
        if fenced.is_empty() {
            return;
        }
        let mut open = Vec::new();
        let mut listed = true;
        for own in &self.own {
            let ways: Vec<Vec<OsString>> = fenced
                .iter()
                .filter(|path| inside(path, own))
                .map(|path| below(path, key(own).len()))
                .collect();
            if ways.is_empty() {
                continue;
            }
            // Links are left out: a link re-opened would re-open where it leads.
            listed &= !net.is_network(own) && down_to(own, &ways, true, &mut open).is_some();
        }
        open.retain(|entry| reopen_safe(entry));
        if listed && open.len() <= MAX_PATHS {
            self.own_open = Some(open);
        } else {
            tracing::warn!("a matter inside this one: the sandbox re-opens none of this matter");
            self.own_open = Some(Vec::new());
            self.own_unlisted = true;
        }
    }

    /// The fence paths of the matters this session is not invited into.
    pub(crate) fn denied_reads(&self) -> impl Iterator<Item = &Path> {
        self.others
            .iter()
            .filter(|other| !other.invited)
            .flat_map(|other| other.paths.iter().map(PathBuf::as_path))
    }

    /// What Codex's profile denies (`agent::codex_fence::Session::other_matters`): the fence
    /// paths of the matters not invited, and each invited matter inside the own one, which Codex
    /// can't make read-only (its turns keep deny entries alone, and the working folder is
    /// writable).
    pub(crate) fn codex_denied(&self) -> Vec<PathBuf> {
        let invited_inside = self.inside_own.iter().filter(|(_, invited)| *invited);
        self.denied_reads()
            .map(Path::to_path_buf)
            .chain(invited_inside.map(|(path, _)| path.clone()))
            .collect()
    }

    /// The fence paths of every other matter.
    pub(crate) fn denied_edits(&self) -> impl Iterator<Item = &Path> {
        self.others
            .iter()
            .flat_map(|other| other.paths.iter().map(PathBuf::as_path))
    }

    /// The folders, each spelling, of the matters this session is invited into.
    pub(crate) fn read_allowed(&self) -> impl Iterator<Item = &Path> {
        self.others
            .iter()
            .filter(|other| other.invited)
            .flat_map(|other| other.spellings.iter().map(PathBuf::as_path))
    }

    /// The folders, each spelling, of the matters this session is not invited into: Claude's
    /// sandbox denies their reads whole (it re-opens the session's own matter inside one).
    pub(crate) fn fenced_folders(&self) -> impl Iterator<Item = &Path> {
        self.others
            .iter()
            .filter(|other| !other.invited)
            .flat_map(|other| other.spellings.iter().map(PathBuf::as_path))
    }
}

/// `folder`'s spellings: where it leads (`touchable`'s walk), then as stored when that differs;
/// as stored alone when it is not absolute-and-local or does not resolve.
fn spellings(folder: &Path, disk: &Disk<'_>) -> Vec<PathBuf> {
    if !folder.is_absolute() {
        return Vec::new();
    }
    let mut out: Vec<PathBuf> = touchable(folder, folder, disk).into_iter().collect();
    if !out.iter().any(|walked| walked == folder) {
        out.push(folder.to_owned());
    }
    out
}

/// `path`'s names for a prefix comparison, folded where the file system folds ([`key_as`]).
pub(crate) fn key(path: &Path) -> Vec<OsString> {
    key_as(path, FOLD_CASE)
}

/// `path`'s names for a prefix comparison, every one folded when `insensitive` (the drive too,
/// as `data_fence::key` folds), and a drive or share prefix in one form: `\\?\C:` is `c:`,
/// `\\?\UNC\srv\s` is `\\srv\s`.
pub(crate) fn key_as(path: &Path, insensitive: bool) -> Vec<OsString> {
    let fold = |text: &str| -> OsString {
        if insensitive {
            OsString::from(fold_name(text))
        } else {
            OsString::from(text)
        }
    };
    path.components()
        .map(|part| match part {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                    fold(&format!("{}:", char::from(drive)))
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => fold(&format!(
                    r"\\{}\{}",
                    server.to_string_lossy(),
                    share.to_string_lossy()
                )),
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) => {
                    fold(&prefix.as_os_str().to_string_lossy())
                }
            },
            Component::Normal(name) => fold(&name.to_string_lossy()),
            other @ (Component::RootDir | Component::CurDir | Component::ParentDir) => {
                other.as_os_str().to_owned()
            }
        })
        .collect()
}

/// The fence paths of one spelling of another matter's folder (the module doc), kept places
/// `keep`: none when it is one of them; `Err` when it holds one and can't be listed down to it.
fn fence_paths(spelling: &Path, keep: &[&Path], net: &NetEnv) -> Result<Vec<PathBuf>, ()> {
    let at = key(spelling);
    let mut inside: Vec<Vec<OsString>> = Vec::new();
    for place in keep {
        let place_key = key(place);
        if !place_key.starts_with(&at) {
            continue;
        }
        if place_key.len() == at.len() {
            return Ok(Vec::new());
        }
        inside.push(below(place, at.len()));
    }
    if inside.is_empty() {
        return Ok(vec![spelling.to_owned()]);
    }
    if net.is_network(spelling) {
        return Err(());
    }
    let mut out = Vec::new();
    down_to(spelling, &inside, false, &mut out).ok_or(())?;
    Ok(out)
}

/// Whether an own matter's entry may be re-opened for Claude's sandbox: UTF-8, its name with no
/// `*` or `?` (Claude Code reads them as a pattern with no escape, `agent::loads::sandbox`) and
/// with a letter or digit, which the sandbox writes as a one-letter class so the entry is a
/// pattern the runtime never resolves through a link swapped in later
/// (`agent::loads::sandbox::reopen_pattern`).
pub(crate) fn reopen_safe(entry: &Path) -> bool {
    let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    entry
        .to_str()
        .is_some_and(|text| !text.contains(['*', '?']))
        && name.chars().any(|c| c.is_ascii_alphanumeric())
}

/// `place`'s names after its first `depth`, as it spells them.
fn below(place: &Path, depth: usize) -> Vec<OsString> {
    place
        .components()
        .skip(depth)
        .map(|part| part.as_os_str().to_owned())
        .collect()
}

/// Every entry of `parent`, and of each folder on the way to each of `ways` (names below
/// `parent`), that is not on a way, into `out`; `None` when a folder on the way can't be listed.
/// With `no_links` (what Claude's sandbox re-opens, [`MatterFence::own_open`]) a link is never
/// put in `out`, and a link on the way is `None`: re-opening a link would re-open where it
/// leads, another matter perhaps. A deny takes links as they are.
fn down_to(
    parent: &Path,
    ways: &[Vec<OsString>],
    no_links: bool,
    out: &mut Vec<PathBuf>,
) -> Option<()> {
    let link = |path: &Path| {
        // One that can't be read counts as a link: it is never re-opened.
        !std::fs::symlink_metadata(path).is_ok_and(|meta| !meta.file_type().is_symlink())
    };
    let fold = |name: &OsString| key(Path::new(name));
    let mut names: Vec<OsString> = std::fs::read_dir(parent)
        .ok()?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<_, _>>()
        .ok()?;
    names.sort();
    for name in names {
        let on_way: Vec<&Vec<OsString>> = ways
            .iter()
            .filter(|way| way.first().is_some_and(|first| fold(first) == fold(&name)))
            .collect();
        if on_way.is_empty() {
            let entry = parent.join(&name);
            if !(no_links && link(&entry)) {
                out.push(entry);
            }
            continue;
        }
        // A kept place itself is left whole; a folder on the way is walked into.
        if on_way.iter().any(|way| way.len() == 1) {
            continue;
        }
        let tails: Vec<Vec<OsString>> = on_way
            .iter()
            .map(|way| way.iter().skip(1).cloned().collect())
            .collect();
        let next = parent.join(&name);
        if no_links && link(&next) {
            return None;
        }
        down_to(&next, &tails, no_links, out)?;
    }
    Some(())
}

/// The innermost other matter `path` lies in by name (each spelling compared folded), unless
/// the session's own matter (each spelling) lies deeper on the way: a refused path there is the
/// session's own. Paths only.
pub(crate) fn innermost<'a>(fence: &'a MatterFence, path: &Path) -> Option<&'a Other> {
    let at = key(path);
    let depth = |spelling: &Path| {
        let spelling = key(spelling);
        at.starts_with(&spelling).then_some(spelling.len())
    };
    let own_depth = fence.own.iter().filter_map(|place| depth(place)).max();
    let (other, deepest) = fence
        .others
        .iter()
        .filter_map(|other| {
            let deepest = other.spellings.iter().filter_map(|s| depth(s)).max()?;
            Some((other, deepest))
        })
        .max_by_key(|(_, deepest)| *deepest)?;
    (own_depth.is_none_or(|own| own < deepest)).then_some(other)
}

#[cfg(test)]
#[path = "matter_fence/tests.rs"]
mod tests;
