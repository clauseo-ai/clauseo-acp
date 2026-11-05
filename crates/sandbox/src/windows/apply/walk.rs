//! The walks a launch runs, and the loop that sets aside the children the lawyer can't open.
//!
//! A walk that meets a child the lawyer can't open ([`ApplyFailure::ChildShut`]) sets it aside
//! ([`Surroundings::set_aside`]) and runs again, until it passes (S15 `PR3b` round 2, N11). Only a
//! child inside the trees the caller names as its own is moved: the launch's own matter, state
//! and notes trees, or the forgotten matter's. One anywhere else (a sibling, the parent's files,
//! another matter's folder) refuses, naming it, and nothing is moved (N9). So does a set-aside
//! that fails, a child that comes back after its set-aside, and a walk past [`MOST_SET_ASIDE`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::Surroundings;
use crate::Error;
use crate::windows::acl_plan::{AdmittedPlan, Inherit, Rights, Sid, in_folder, verbatim};
use crate::windows::admit::PathKind;
use crate::windows::ledger::Entry;
use crate::windows::writer::{AclWriter, ApplyFailure, Reach, Strict, Walked};

/// The most children one walk sets aside before it refuses.
pub(super) const MOST_SET_ASIDE: usize = 64;

/// The launch's own kinds of folder: what its agent writes.
pub(super) const OWN_KINDS: [PathKind; 3] = [PathKind::Matter, PathKind::State, PathKind::Notes];

/// Why a step refused: the kind of path, the failure, and the child it names, if any.
#[derive(Debug)]
pub(super) struct Refused {
    pub(super) kind: PathKind,
    pub(super) failure: ApplyFailure,
    pub(super) at: Option<PathBuf>,
}

impl Refused {
    /// A refusal naming no child.
    pub(super) fn new(kind: PathKind, failure: ApplyFailure) -> Refused {
        Refused {
            kind,
            failure,
            at: None,
        }
    }

    /// The error the caller returns.
    pub(super) fn error(self) -> Error {
        Error::AclApply {
            path_kind: self.kind,
            failure: self.failure,
            at: self.at,
        }
    }
}

/// Runs `walk` until it passes, setting aside each child the lawyer can't open inside one of
/// `own`; see the module's docs.
pub(super) fn clearing(
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
    own: &[PathBuf],
    kind: PathKind,
    mut walk: impl FnMut(&mut dyn AclWriter) -> Result<Walked, ApplyFailure>,
) -> Result<Walked, Refused> {
    let mut moved: Vec<PathBuf> = Vec::new();
    loop {
        match walk(acl) {
            Ok(walked) => return Ok(walked),
            Err(ApplyFailure::ChildShut) => {
                let shut = acl.refused_at();
                let refuse = |at| Refused {
                    kind,
                    failure: ApplyFailure::ChildShut,
                    at,
                };
                let Some(shut) = shut else {
                    return Err(refuse(None));
                };
                // The walk reports the child by its final path, verbatim (`\\?\C:\…`);
                // the roots are the policy's plain paths. Both are compared verbatim, whatever
                // their length (round 5), and the child must be strictly inside a root, never
                // the root itself. It is set aside and named as the walk reported it.
                let inside = own
                    .iter()
                    .any(|root| in_folder(&shut, root) && !in_folder(root, &shut));
                let again = moved.contains(&verbatim(&shut));
                if !inside || again || moved.len() >= MOST_SET_ASIDE {
                    return Err(refuse(Some(shut)));
                }
                if let Err(error) = around.set_aside(&shut) {
                    let error = error.kind();
                    tracing::warn!(%error, "a child the lawyer can't open couldn't be set aside");
                    return Err(refuse(Some(shut)));
                }
                moved.push(verbatim(&shut));
            }
            Err(failure) => {
                return Err(Refused {
                    kind,
                    failure,
                    at: acl.refused_at(),
                });
            }
        }
    }
}

/// The launch's own matter, state and notes trees: `plan`'s `Modify` grants of those kinds for
/// its own matter. Where a child the lawyer can't open may be set aside.
pub(super) fn own_roots(plan: &AdmittedPlan) -> Vec<PathBuf> {
    plan.grants()
        .iter()
        .zip(plan.owners())
        .filter(|(g, owner)| {
            **owner == plan.matter()
                && g.rights() == Rights::Modify
                && OWN_KINDS.contains(&g.kind())
        })
        .map(|(g, _)| g.path().to_owned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// What a strict walk of `plan`'s trees holds children to: the SIDs its denies name, and every
/// pool SID it names (the owners whose children are repaired).
pub(super) fn strict_for(plan: &AdmittedPlan) -> Strict {
    let items: Vec<(Rights, &Sid)> = plan
        .grants()
        .iter()
        .map(|g| (g.rights(), g.sid()))
        .collect();
    strict_from(&items)
}

/// [`strict_for`] of a matter's ledger entries.
pub(super) fn strict_of(entries: &[Entry]) -> Strict {
    let items: Vec<(Rights, &Sid)> = entries.iter().map(|e| (e.rights, &e.sid)).collect();
    strict_from(&items)
}

fn strict_from(items: &[(Rights, &Sid)]) -> Strict {
    let sids = |keep: &dyn Fn(Rights) -> bool| -> Vec<Sid> {
        items
            .iter()
            .filter(|(r, _)| keep(*r))
            .map(|(_, s)| (*s).clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    Strict {
        watched: sids(&|r| matches!(r, Rights::DenyReadWrite | Rights::DenyRead)),
        pool: sids(&|r| !r.is_label()),
    }
}

/// One walk to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Walk {
    pub(super) root: PathBuf,
    pub(super) reach: Reach,
    pub(super) strict: bool,
    pub(super) kind: PathKind,
}

/// The walks `plan` needs, shallowest first: each path with an inheritable grant `unwalked`
/// names (this run wrote its entry, or its entry was still pending), and the launch's own
/// matter, state and notes folders always. A tree inside another walked tree is left to that
/// walk. A tree is strict when it is one of the launch's own folders or carries a deny.
pub(super) fn walks(plan: &AdmittedPlan, unwalked: impl Fn(usize) -> bool) -> Vec<Walk> {
    let mut found: BTreeMap<(usize, PathBuf), Walk> = BTreeMap::new();
    for (i, (grant, owner)) in plan.grants().iter().zip(plan.owners()).enumerate() {
        if grant.inherit() == Inherit::ThisOnly {
            continue;
        }
        let own = *owner == plan.matter()
            && grant.rights() == Rights::Modify
            && OWN_KINDS.contains(&grant.kind());
        if !own && !unwalked(i) {
            continue;
        }
        let reach = reach_of(grant.inherit());
        let key = (grant.path().components().count(), grant.path().to_owned());
        let walk = found.entry(key).or_insert_with(|| Walk {
            root: grant.path().to_owned(),
            reach,
            strict: false,
            kind: grant.kind(),
        });
        walk.strict |= own || grant.rights().is_deny();
        if reach == Reach::Tree {
            walk.reach = Reach::Tree;
        }
    }
    let all: Vec<Walk> = found.into_values().collect();
    all.iter()
        .filter(|w| {
            !all.iter().any(|outer| {
                outer.reach == Reach::Tree
                    && outer.root != w.root
                    && in_folder(&w.root, &outer.root)
                    && (outer.strict || !w.strict)
            })
        })
        .cloned()
        .collect()
}

/// How far an entry's inheritance reaches.
pub(super) fn reach_of(inherit: Inherit) -> Reach {
    match inherit {
        Inherit::FilesInFolder => Reach::FilesOnly,
        Inherit::Tree | Inherit::ThisOnly => Reach::Tree,
    }
}

/// Whether `path` is inside one of `roots` (or is one).
pub(super) fn inside_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| in_folder(path, root))
}
