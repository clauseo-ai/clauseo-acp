//! What a denial's card may offer (R18): one pure function, [`offer`], over canonical paths,
//! run when the card is logged and again (3b) when the lawyer allows it.
//!
//! - Never offered: the home folder or a folder holding it, a root (`/`, a drive), Clauseo's
//!   data folder, a refused folder or tree (the firm's, the host's), a path with a `.git` or any
//!   other dot-named component, a credential store, this matter or a folder overlapping it, a
//!   link target, a path the sandbox classed `ReadOnly`, `Hidden` (unless another matter's) or
//!   `LinkTarget`, and a site refused for anything but `NotAllowed` (`PrivateAddress` above all).
//! - Another matter (matched against the registry): the asked path alone, read-only, never its
//!   root and never anything under its dot folders (`.clauseo`, `.claude`, `.codex`, `.agents`,
//!   `.git`).
//! - A file whose folder is never offered (one directly in home): a card for that file alone,
//!   unless the file itself falls under a rule above.
//!
//! The folder rules are those of `Policy::check_allowed_folder`
//! (`crates/sandbox/src/policy/build.rs`), which the host can't call until PR-D builds a
//! policy per launch; they are repeated here, stricter (refused folders count as trees, dot
//! names and credential stores are refused).

use std::path::{Component, Path, PathBuf};

use clauseo_protocol::{DenialKind, MatterId, OtherMatter};
use clauseo_sandbox::{HostRefusal, PathClass};

use unicode_normalization::UnicodeNormalization as _;

use super::network::NetEnv;

#[cfg(test)]
mod tests;

/// Folder names that hold keys and passwords where the name has no leading dot: macOS
/// keychains, the Linux keyrings, Windows' credential, protect and vault stores. Compared
/// folded ([`fold_name`]), on every system.
const CREDENTIAL_STORES: [&str; 5] = ["Keychains", "keyrings", "Credentials", "Protect", "Vault"];

/// A matter the registry knows, for matching a refused path to another matter.
#[derive(Debug, Clone)]
pub(crate) struct KnownMatter {
    /// The matter.
    pub(crate) id: MatterId,
    /// Its name, for the card (display only).
    pub(crate) name: String,
    /// Its folder, canonical.
    pub(crate) root: PathBuf,
}

/// What the rules compare a refused path against; every path canonical.
#[derive(Debug, Clone)]
pub(crate) struct OfferRules {
    /// The lawyer's home folder.
    pub(crate) home: Option<PathBuf>,
    /// Clauseo's data folder.
    pub(crate) data_dir: PathBuf,
    /// The matter whose agent was refused, and its folder.
    pub(crate) matter: MatterId,
    pub(crate) matter_root: PathBuf,
    /// Refused folders and trees: never offered, nor any folder above or inside one. The home
    /// folder, when listed, is left out here: [`OfferRules::home`] has its own rule.
    pub(crate) refused: Vec<PathBuf>,
    /// Every matter the host knows, this one included (it is skipped).
    pub(crate) matters: Vec<KnownMatter>,
    /// Whether names compare without case (macOS and Windows volumes).
    pub(crate) fold_case: bool,
}

/// One refusal, as the rules read it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Refused<'a> {
    /// A path: canonical, whether it is a file (or does not exist), and the sandbox's class.
    Path {
        path: &'a Path,
        is_file: bool,
        class: PathClass,
    },
    /// A site the egress proxy refused.
    Site {
        host: &'a str,
        shared_hosting: bool,
        why: HostRefusal,
    },
    /// The upstream proxy wanted a sign-in.
    UpstreamAuth { upstream: &'a str },
    /// A path that names a network location (`network.rs`) or could not be resolved, as the
    /// agent wrote it: never offered.
    Unchecked { path: &'a str },
}

/// What the card shows and whether it may widen it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Offer {
    pub(crate) kind: DenialKind,
    pub(crate) offered: bool,
}

/// The card for `refused` under `rules`. Pure: no disk, no clock. A path must already be
/// resolved (`resolve.rs`); a network or unresolved one comes as [`Refused::Unchecked`].
/// Another matter is found by matching the registry; once stage C's #735 adds
/// `PathClass::OtherMatter { matter, refused }` to the sandbox's `classify`, this can take that
/// class instead and keep the rest.
pub(crate) fn offer(rules: &OfferRules, refused: Refused<'_>) -> Offer {
    match refused {
        Refused::Path {
            path,
            is_file,
            class,
        } => path_offer(rules, path, is_file, class),
        Refused::Site {
            host,
            shared_hosting,
            why,
        } => Offer {
            kind: DenialKind::Site {
                host: host.to_owned(),
                shared_hosting,
            },
            offered: why == HostRefusal::NotAllowed,
        },
        Refused::Unchecked { path } => Offer {
            kind: DenialKind::File {
                path: path.to_owned(),
                other_matter: None,
            },
            offered: false,
        },
        Refused::UpstreamAuth { upstream } => Offer {
            kind: DenialKind::UpstreamAuth {
                upstream: upstream.to_owned(),
            },
            offered: false,
        },
    }
}

fn path_offer(rules: &OfferRules, path: &Path, is_file: bool, class: PathClass) -> Offer {
    let folder = if is_file {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    let shown = |p: &Path| p.display().to_string();
    if let Some(other) = rules.other_matter(path) {
        let other_matter = Some(OtherMatter {
            matter: other.id,
            name: other.name.clone(),
        });
        // Until #735 the sandbox hides other matters, so their refusals come as `Hidden`.
        let class_ok = matches!(class, PathClass::Outside | PathClass::Hidden);
        let offered = class_ok
            && !rules.same(path, &other.root)
            && !rules.never(path)
            && !rules.overlaps_this_matter(path);
        let kind = if is_file {
            DenialKind::File {
                path: shown(path),
                other_matter,
            }
        } else {
            DenialKind::Folder {
                path: shown(path),
                other_matter,
            }
        };
        return Offer { kind, offered };
    }
    let outside = class == PathClass::Outside;
    if outside && rules.folder_offerable(folder) {
        let kind = DenialKind::Folder {
            path: shown(folder),
            other_matter: None,
        };
        return Offer {
            kind,
            offered: true,
        };
    }
    if is_file && outside && !rules.never(path) && !rules.overlaps_this_matter(path) {
        let kind = DenialKind::File {
            path: shown(path),
            other_matter: None,
        };
        return Offer {
            kind,
            offered: true,
        };
    }
    Offer {
        kind: DenialKind::Folder {
            path: shown(folder),
            other_matter: None,
        },
        offered: false,
    }
}

impl OfferRules {
    /// The other matter `path` is in (the innermost, when matters nest), if any.
    fn other_matter(&self, path: &Path) -> Option<&KnownMatter> {
        self.matters
            .iter()
            .filter(|known| known.id != self.matter && self.under(path, &known.root))
            .max_by_key(|known| known.root.components().count())
    }

    /// Whether `folder` may be offered whole: `check_allowed_folder`'s rules, then the host's.
    fn folder_offerable(&self, folder: &Path) -> bool {
        !is_root(folder)
            && !self
                .home
                .as_deref()
                .is_some_and(|home| self.under(home, folder))
            && !self.overlaps_this_matter(folder)
            && !self
                .matters
                .iter()
                .any(|known| self.under(&known.root, folder))
            && !self.refused.iter().any(|r| self.under(r, folder))
            && !self.never(folder)
    }

    /// The rules no path passes, whatever it is: a root, the data folder or a refused tree (at
    /// or inside), a dot-named component (`.git`, `.ssh`, another matter's `.claude`), a
    /// credential store, or the home folder itself.
    fn never(&self, path: &Path) -> bool {
        is_root(path)
            || self.overlap(path, &self.data_dir)
            || self.refused.iter().any(|r| self.under(path, r))
            || self
                .home
                .as_deref()
                .is_some_and(|home| self.same(path, home))
            || path.components().any(|c| match c {
                Component::Normal(name) => {
                    let name = fold_name(&name.to_string_lossy());
                    name.starts_with('.')
                        || CREDENTIAL_STORES
                            .iter()
                            .any(|store| name == fold_name(store))
                }
                Component::Prefix(_)
                | Component::RootDir
                | Component::CurDir
                | Component::ParentDir => false,
            })
    }

    fn overlaps_this_matter(&self, path: &Path) -> bool {
        self.overlap(path, &self.matter_root)
    }

    /// Whether `a` is `b`, inside it, or holds it.
    fn overlap(&self, a: &Path, b: &Path) -> bool {
        self.under(a, b) || self.under(b, a)
    }

    fn same(&self, a: &Path, b: &Path) -> bool {
        a.components().count() == b.components().count() && self.under(a, b)
    }

    fn under(&self, path: &Path, prefix: &Path) -> bool {
        under(path, prefix, self.fold_case)
    }

    /// The rules with every path made canonical (on the disk: off the actor), where it
    /// resolves, and the home folder left out of the refused list. These are the host's own
    /// paths, not the agent's; one that names a network location (a matter on a share) is
    /// still kept as written, so a check never signs in to a share.
    pub(crate) fn canonical(self, net: &NetEnv) -> Self {
        let fold = |p: PathBuf| {
            if net.is_network(&p) {
                p
            } else {
                dunce::canonicalize(&p).unwrap_or(p)
            }
        };
        let home = self.home.map(fold);
        let fold_case = self.fold_case;
        let refused = self
            .refused
            .into_iter()
            .map(fold)
            .filter(|r| {
                !home
                    .as_deref()
                    .is_some_and(|home| under(r, home, fold_case) && under(home, r, fold_case))
            })
            .collect();
        let matters = self
            .matters
            .into_iter()
            .map(|known| KnownMatter {
                root: fold(known.root),
                ..known
            })
            .collect();
        Self {
            home,
            data_dir: fold(self.data_dir),
            matter: self.matter,
            matter_root: fold(self.matter_root),
            refused,
            matters,
            fold_case,
        }
    }
}

/// Whether `path` is `prefix` or inside it, by whole components; without case when
/// `fold_case`.
pub(crate) fn under(path: &Path, prefix: &Path, fold_case: bool) -> bool {
    if !fold_case {
        return path.starts_with(prefix);
    }
    let mut names = path.components();
    prefix.components().all(|want| {
        names.next().is_some_and(|got| {
            let (got, want) = (got.as_os_str(), want.as_os_str());
            got == want || fold_name(&got.to_string_lossy()) == fold_name(&want.to_string_lossy())
        })
    })
}

/// A root: `/`, a drive (`C:\`) or a share's root.
fn is_root(path: &Path) -> bool {
    path.parent().is_none()
}

/// `name` case-folded for comparing names on a path: lower case, plus the two characters whose
/// simple case fold is an ASCII letter though their lower case is not (APFS and NTFS's upcase
/// table fold them too): `ſ` (U+017F) to `s` and the Kelvin sign `K` (U+212A) to `k`. So
/// `.clauſeo` is `.clauseo` and `\u{212A}eychains` is `keychains`. Used for every refused name,
/// on every system: it only ever matches more. Both sides go to NFD first, as APFS compares
/// names, so a name typed composed matches one stored decomposed.
pub(crate) fn fold_name(name: &str) -> String {
    let folded: String = name
        .nfd()
        .flat_map(|c| match c {
            '\u{17F}' => 's'.to_lowercase(),
            '\u{212A}' => 'k'.to_lowercase(),
            other => other.to_lowercase(),
        })
        .collect();
    folded.nfd().collect()
}

/// Folds the case of names on the volumes that ignore it: macOS and Windows.
pub(crate) const FOLD_CASE: bool = cfg!(any(target_os = "macos", windows));
