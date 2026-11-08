//! Which folder a resolved path really is, by file identity (H1, H2 of the #764 review): Windows
//! and macOS spell one folder more than one way (`Other.`, `rohan `, `CLAUSE~1`,
//! `/System/Volumes/Data/Users/<me>`), so the text rules alone can be walked around. Once the
//! walk has proved a path local, [`Places::pin`] asks the identity of every folder on it and
//! spells the deepest one that is a known place (home, the data folder, a matter root and its
//! protected folders, `.ssh`, a keychain or credential store, a refused folder, or a folder above
//! one of them) as that place is spelled; the text rules then judge that spelling. Identity
//! decides; text is the first filter.
//!
//! A whole folder is also refused by identity ([`Places::holds_protected`]): one that is a known
//! place or holds one (`/Users`, `C:\Users`, a folder holding another matter, macOS's
//! `/System/Volumes`), or the root of a mounted volume (`/System/Volumes/Data`, `/`, `C:\`).
//!
//! The places are captured once per session ([`Captured`]) behind the [`Identity`] trait, which
//! `crate::network_paths`' identity primitive answers (#766: `st_dev` and `st_ino` on Unix, the
//! volume serial and file ID on Windows, never following a link); tests use a fake that maps
//! aliases to one ID. Every host path is walked first, as an agent's is ([`resolve::resolve`]):
//! one the walk refuses (a matter root that is a link to a share) is kept as written, judged by
//! its text, and never opened.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use super::{OfferRules, fold_name, under};
use crate::actor::turn::denials::resolve::{self, Disk, Resolved};
use crate::network_paths::ascii_fold::REFUSED_IN_OTHER_MATTER;
use crate::network_paths::resolve::{Fs as _, RealFs};

#[cfg(test)]
pub(crate) mod tests;

/// One file or folder, whatever it is called: its volume and its ID on that volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FileId {
    pub(crate) volume: u64,
    pub(crate) file: u128,
}

/// Where identities come from: the name at a path itself, never following a link there.
pub(crate) trait Identity {
    /// The identity of `path`, a local path the walk proved. An error when it can't be read:
    /// `NotFound` for a missing name; any other is a refusal where a decision rests on it.
    fn id(&self, path: &Path) -> io::Result<FileId>;
}

/// The disk's own answer, from `network_paths`' primitive (`RealFs::identity`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct RealIdentity;

impl Identity for RealIdentity {
    fn id(&self, path: &Path) -> io::Result<FileId> {
        let (volume, file) = RealFs.identity(path)?;
        Ok(FileId { volume, file })
    }
}

/// The places under home that hold keys and passwords: never offered, so captured to catch a
/// respelling of them.
const CREDENTIAL_PLACES: [&str; 10] = [
    ".ssh",
    ".gnupg",
    "Library/Keychains",
    ".local/share/keyrings",
    "AppData/Roaming/Microsoft/Credentials",
    "AppData/Roaming/Microsoft/Protect",
    "AppData/Roaming/Microsoft/Vault",
    "AppData/Local/Microsoft/Credentials",
    "AppData/Local/Microsoft/Vault",
    "AppData/Local/Microsoft/Protect",
];

/// The system's own keychains.
const SYSTEM_CREDENTIAL_PLACES: [&str; 1] = ["/Library/Keychains"];

/// macOS's data volume, where a firmlinked place (`/Users/a`) also lies
/// (`/System/Volumes/Data/Users/a`): every folder above that spelling holds the place too.
const MAC_DATA_VOLUME: &str = "/System/Volumes/Data";

/// The known places by identity, each with the spelling the rules use.
#[derive(Debug, Clone, Default)]
pub(crate) struct Places {
    known: Vec<(FileId, PathBuf)>,
    /// Every protected place and every folder holding one, whatever its spelling: never offered
    /// whole.
    holders: HashSet<FileId>,
    /// Places the walk proved local that were missing at capture, in a folder that was there (a
    /// `.ssh` not made yet): one that appears makes the capture stale ([`Places::stale`]).
    missing: Vec<PathBuf>,
}

impl Places {
    /// The identities of `rules`' places (walked rules) and of every folder above each, asked of
    /// `ids` once each is walked on `disk` (only its walked path is asked). A place the walk
    /// refuses (a network location, a link to one, anything unresolved) is never asked, nor is
    /// anything under it: its text alone judges it. On macOS the folders above each place's
    /// data volume spelling are holders too.
    pub(crate) fn capture(rules: &OfferRules, disk: &Disk<'_>, ids: &dyn Identity) -> Self {
        let mut wanted: Vec<PathBuf> = Vec::new();
        let mut want = |place: PathBuf| {
            for above in place.ancestors() {
                if !above.as_os_str().is_empty() && !wanted.iter().any(|w| w == above) {
                    wanted.push(above.to_path_buf());
                }
            }
        };
        if let Some(home) = &rules.home {
            want(home.clone());
            for place in CREDENTIAL_PLACES {
                want(home.join(place));
            }
        }
        for place in SYSTEM_CREDENTIAL_PLACES {
            want(PathBuf::from(place));
        }
        want(rules.data_dir.clone());
        let roots = rules.matters.iter().map(|known| &known.root);
        for root in std::iter::once(&rules.matter_root).chain(roots) {
            want(root.clone());
            for name in REFUSED_IN_OTHER_MATTER {
                want(root.join(name));
            }
        }
        for refused in &rules.refused {
            want(refused.clone());
        }
        let mut places = Self::default();
        let mut found: Vec<PathBuf> = Vec::new();
        let mut absent: Vec<PathBuf> = Vec::new();
        for place in &wanted {
            let Resolved::Local { path: walked, .. } = resolve::resolve(place, disk) else {
                continue;
            };
            match ids.id(&walked) {
                Ok(id) => {
                    places.known.push((id, place.clone()));
                    places.holders.insert(id);
                    found.push(walked);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => absent.push(walked),
                Err(_) => {}
            }
        }
        places.missing = absent
            .into_iter()
            .filter(|walked| {
                walked
                    .parent()
                    .is_some_and(|up| found.iter().any(|f| f == up))
            })
            .collect();
        if disk.net.macos {
            for place in &wanted {
                let Ok(inside) = place.strip_prefix("/") else {
                    continue;
                };
                for above in Path::new(MAC_DATA_VOLUME).join(inside).ancestors() {
                    if let Resolved::Local { path, .. } = resolve::resolve(above, disk)
                        && let Ok(id) = ids.id(&path)
                    {
                        places.holders.insert(id);
                    }
                }
            }
        }
        places
    }

    /// Whether a place missing at capture is there now: the capture is stale, taken again. Each
    /// is asked by its walked path, whose folder the walk proved local; a link made there since
    /// is asked about itself, never followed.
    pub(crate) fn stale(&self, ids: &dyn Identity) -> bool {
        self.missing.iter().any(|walked| ids.id(walked).is_ok())
    }

    /// Whether `folder` (resolved and local) may never be offered whole, by identity: it is a
    /// known place or holds one, or it is the root of a mounted volume (no folder above it, or
    /// its volume is not its parent's). A missing folder holds nothing; one whose identity, or
    /// its parent's, can't be read is refused.
    pub(crate) fn holds_protected(&self, folder: &Path, ids: &dyn Identity) -> bool {
        let Some(parent) = folder.parent().filter(|up| !up.as_os_str().is_empty()) else {
            return true;
        };
        let id = match ids.id(folder) {
            Ok(id) => id,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return false,
            Err(_) => return true,
        };
        if self.holders.contains(&id) {
            return true;
        }
        ids.id(parent).map_or(true, |up| up.volume != id.volume)
    }

    /// `path` (resolved and local) with the deepest folder on it whose identity is a known
    /// place spelled as that place is, the rest after it as written; as it is when none is. An
    /// error when an identity on the way can't be read (other than a missing name): the path is
    /// then refused as unresolved, since it might be a place.
    pub(crate) fn pin(&self, path: &Path, ids: &dyn Identity) -> io::Result<PathBuf> {
        if self.known.is_empty() {
            return Ok(path.to_path_buf());
        }
        for prefix in path.ancestors() {
            if prefix.as_os_str().is_empty() {
                continue;
            }
            let id = match ids.id(prefix) {
                Ok(id) => id,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if let Some((_, place)) = self.known.iter().find(|(known, _)| *known == id) {
                let rest = path.strip_prefix(prefix).unwrap_or_else(|_| Path::new(""));
                return Ok(if rest.as_os_str().is_empty() {
                    place.clone()
                } else {
                    place.join(rest)
                });
            }
        }
        Ok(path.to_path_buf())
    }

    /// How many places were captured (tests).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.known.len()
    }
}

/// What a card or a refusal is about: its identity when it has one, and its text (folded on a
/// folding volume) for one that has none (a missing name, a site).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) id: Option<FileId>,
    pub(crate) text: String,
}

impl Target {
    /// `path`'s target: asked of `ids` (only for a path proved local), its text folded when
    /// `fold_case`.
    pub(crate) fn of(path: &Path, ids: Option<&dyn Identity>, fold_case: bool) -> Self {
        Self {
            id: ids.and_then(|ids| ids.id(path).ok()),
            text: Self::key(&path.display().to_string(), fold_case),
        }
    }

    /// A target known by its text alone (a site, a path never touched).
    pub(crate) fn text(text: &str, fold_case: bool) -> Self {
        Self {
            id: None,
            text: Self::key(text, fold_case),
        }
    }

    fn key(text: &str, fold_case: bool) -> String {
        if fold_case {
            fold_name(text)
        } else {
            text.to_owned()
        }
    }

    /// The same target for a dedupe: the same identity when both have one, else the same text.
    pub(crate) fn same(&self, other: &Self) -> bool {
        match (self.id, other.id) {
            (Some(a), Some(b)) => a == b,
            _ => self.text == other.text,
        }
    }

    /// The same file by identity, both known: what confirms a tool call's claim on another
    /// matter, and what R18 3b asks at grant time. 3b resolves the card's path again, pins it
    /// ([`Places::pin`]), takes its [`Target::of`], and grants only when this holds against the
    /// card's own target (`Denials::card_target`): a card's path string never grants.
    pub(crate) fn same_file(&self, other: &Self) -> bool {
        matches!((self.id, other.id), (Some(a), Some(b)) if a == b)
    }
}

/// One session's rules made canonical and its places captured: done once per session (and
/// again only when the host's rules change), off the actor.
#[derive(Debug)]
pub(crate) struct Captured {
    pub(crate) rules: OfferRules,
    pub(crate) places: Places,
}

impl Captured {
    /// `raw`'s paths walked on `disk` ([`walked`]), then their identities asked of `ids`.
    pub(crate) fn new(raw: OfferRules, disk: &Disk<'_>, ids: &dyn Identity) -> Self {
        let rules = walked(raw, disk);
        let places = Places::capture(&rules, disk, ids);
        Self { rules, places }
    }
}

/// `raw` with every path walked on `disk` ([`resolve::resolve`], network-safe, as an agent's
/// path is), and the home folder left out of the refused list. These are the host's own paths,
/// but one may still be a link to a share (a matter kept there): a path the walk refuses is
/// kept as written, so the capture never opens it.
pub(crate) fn walked(raw: OfferRules, disk: &Disk<'_>) -> OfferRules {
    let walk = |p: PathBuf| match resolve::resolve(&p, disk) {
        Resolved::Local { path, .. } => path,
        Resolved::Network | Resolved::Unresolved => p,
    };
    let home = raw.home.map(walk);
    let fold_case = raw.fold_case;
    let refused = raw
        .refused
        .into_iter()
        .map(walk)
        .filter(|r| {
            !home
                .as_deref()
                .is_some_and(|home| under(r, home, fold_case) && under(home, r, fold_case))
        })
        .collect();
    let matters = raw
        .matters
        .into_iter()
        .map(|known| super::KnownMatter {
            root: walk(known.root),
            ..known
        })
        .collect();
    OfferRules {
        home,
        data_dir: walk(raw.data_dir),
        matter: raw.matter,
        matter_root: walk(raw.matter_root),
        refused,
        matters,
        fold_case,
    }
}
