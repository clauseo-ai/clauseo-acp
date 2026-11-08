//! Which folder a resolved path really is, by file identity (H1, H2 of the #764 review): Windows
//! and macOS spell one folder more than one way (`Other.`, `rohan `, `CLAUSE~1`,
//! `/System/Volumes/Data/Users/<me>`), so the text rules alone can be walked around. Once the
//! walk has proved a path local, [`Places::pin`] asks the identity of every folder on it and
//! spells the deepest one that is a known place (home, the data folder, a matter root and its
//! protected folders, `.ssh`, a keychain or credential store, a refused folder, or a folder above
//! one of them) as that place is spelled; the text rules then judge that spelling. Identity
//! decides; text is the first filter.
//!
//! The places are captured once per session ([`Captured`]) behind the [`Identity`] trait, which
//! `crate::network_paths`' identity primitive answers (#766: `st_dev` and `st_ino` on Unix, the
//! volume serial and file ID on Windows, never following a link); tests use a fake that maps
//! aliases to one ID.

use std::path::{Path, PathBuf};

use super::names::REFUSED_IN_OTHER_MATTER_NAMES;
use super::{OfferRules, fold_name};
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
    /// The identity of `path`, a local path the walk proved; `None` when it can't be read (a
    /// missing name, an error, or a system with no source yet).
    fn id(&self, path: &Path) -> Option<FileId>;
}

/// The disk's own answer, from `network_paths`' primitive (`RealFs::identity`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct RealIdentity;

impl Identity for RealIdentity {
    fn id(&self, path: &Path) -> Option<FileId> {
        let (volume, file) = RealFs.identity(path)?;
        Some(FileId { volume, file })
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

/// The known places by identity, each with the spelling the rules use.
#[derive(Debug, Clone, Default)]
pub(crate) struct Places {
    known: Vec<(FileId, PathBuf)>,
}

impl Places {
    /// The identities of `rules`' places (canonical rules) and of every folder above each, asked
    /// of `ids`. A place `is_network` calls a network location (a matter on a share) is never
    /// asked: its text alone judges it.
    pub(crate) fn capture(
        rules: &OfferRules,
        ids: &dyn Identity,
        is_network: &dyn Fn(&Path) -> bool,
    ) -> Self {
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
        want(rules.matter_root.clone());
        let roots = rules.matters.iter().map(|known| &known.root);
        for root in std::iter::once(&rules.matter_root).chain(roots) {
            want(root.clone());
            for name in REFUSED_IN_OTHER_MATTER_NAMES {
                want(root.join(name));
            }
        }
        for refused in &rules.refused {
            want(refused.clone());
        }
        let known = wanted
            .into_iter()
            .filter(|place| !is_network(place))
            .filter_map(|place| Some((ids.id(&place)?, place)))
            .collect();
        Self { known }
    }

    /// `path` (resolved and local) with the deepest folder on it whose identity is a known
    /// place spelled as that place is, the rest after it as written; as it is when none is.
    pub(crate) fn pin(&self, path: &Path, ids: &dyn Identity) -> PathBuf {
        if self.known.is_empty() {
            return path.to_path_buf();
        }
        for prefix in path.ancestors() {
            if prefix.as_os_str().is_empty() {
                continue;
            }
            let Some(id) = ids.id(prefix) else {
                continue;
            };
            if let Some((_, place)) = self.known.iter().find(|(known, _)| *known == id) {
                let rest = path.strip_prefix(prefix).unwrap_or_else(|_| Path::new(""));
                return if rest.as_os_str().is_empty() {
                    place.clone()
                } else {
                    place.join(rest)
                };
            }
        }
        path.to_path_buf()
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
            id: ids.and_then(|ids| ids.id(path)),
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
    /// `raw`'s paths made canonical where they resolve (a network one kept as written), then
    /// their identities asked of `ids`.
    pub(crate) fn new(
        raw: OfferRules,
        ids: &dyn Identity,
        is_network: &dyn Fn(&Path) -> bool,
    ) -> Self {
        let rules = raw.canonical(is_network);
        let places = Places::capture(&rules, ids, is_network);
        Self { rules, places }
    }
}
