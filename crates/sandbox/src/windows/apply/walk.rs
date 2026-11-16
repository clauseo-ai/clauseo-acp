//! The walks a launch runs, and the loop that sets aside the children the lawyer can't open.
//!
//! A walk that meets a child the lawyer can't open ([`ApplyFailure::ChildShut`]) sets it aside
//! ([`Surroundings::set_aside`]) and runs again, until it passes (S15 `PR3b` round 2, N11). Only a
//! child inside a matter, state or notes tree the caller names is moved, and into the set-aside
//! of the matter that tree belongs to: the launch's own trees, each registered matter's trees
//! whose fence the launch applies (round 6: otherwise one matter's agent could plant a child
//! that blocks other matters' launches), or the forgotten matter's. One anywhere else (a
//! sibling, the parent's files) refuses, naming it, and nothing is moved (N9). So does a
//! set-aside that fails, a child that comes back after its set-aside, and a walk past
//! [`MOST_SET_ASIDE`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::Surroundings;
use crate::Error;
use crate::windows::acl_plan::{AdmittedPlan, Inherit, Rights, Sid, in_folder, verbatim};
use crate::windows::admit::PathKind;
use crate::windows::ledger::Entry;
use crate::windows::writer::{AclWriter, ApplyFailure, Reach, Strict, Walked};

/// The most children one walk sets aside before it refuses.
pub(super) const MOST_SET_ASIDE: usize = 64;

/// The launch's own kinds of folder: what its agent writes.
pub(crate) const OWN_KINDS: [PathKind; 3] = [PathKind::Matter, PathKind::State, PathKind::Notes];

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

/// A matter, state or notes tree, and the matter it belongs to: whose set-aside takes a child
/// in it the lawyer can't open.
pub(super) type OwnRoot = (PathBuf, MatterId);

/// Runs `walk` until it passes, setting aside each child the lawyer can't open inside one of
/// `own`, into the set-aside of the matter that tree belongs to; see the module's docs.
pub(super) fn clearing(
    acl: &mut dyn AclWriter,
    around: &mut dyn Surroundings,
    own: &[OwnRoot],
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
                // the root itself. It is set aside, named as the walk reported it, by the matter
                // whose tree holds it most closely.
                let owner = own
                    .iter()
                    .filter(|(root, _)| in_folder(&shut, root) && !in_folder(root, &shut))
                    .max_by_key(|(root, _)| root.components().count())
                    .map(|(_, owner)| *owner);
                let again = moved.contains(&verbatim(&shut));
                let Some(owner) = owner.filter(|_| !again && moved.len() < MOST_SET_ASIDE) else {
                    return Err(refuse(Some(shut)));
                };
                if let Err(error) = around.set_aside(owner, &shut) {
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

/// The matter, state and notes trees `plan` names, each with the matter it belongs to: the
/// launch's own (its `Modify` grants of those kinds) and every registered matter's whose fence
/// it applies (the fence's read and write denies, owned by the matter they fence, as the plan
/// and the ledger record them). Where a child the lawyer can't open may be set aside, and whose
/// set-aside takes it.
pub(super) fn own_roots(plan: &AdmittedPlan) -> Vec<OwnRoot> {
    plan.grants()
        .iter()
        .zip(plan.owners())
        .filter(|(g, owner)| {
            let own = **owner == plan.matter() && g.rights() == Rights::Modify;
            let fence = g.rights() == Rights::DenyReadWrite;
            (own || fence) && OWN_KINDS.contains(&g.kind())
        })
        .map(|(g, owner)| (g.path().to_owned(), *owner))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// What a strict walk of `plan`'s trees holds children to: the SIDs its denies name, and every
/// pool SID it names (the owners whose children are repaired).
pub(crate) fn strict_for(plan: &AdmittedPlan) -> Strict {
    let items: Vec<(Rights, &Sid)> = plan
        .grants()
        .iter()
        .map(|g| (g.rights(), g.sid()))
        .collect();
    strict_from(&items)
}

/// [`strict_for`] of a matter's ledger entries, for a walk of its own trees: with its subjects
/// (the SIDs of its own folders' `Modify` entries) as the assigned ones.
pub(crate) fn strict_of(entries: &[Entry]) -> Strict {
    let items: Vec<(Rights, &Sid)> = entries.iter().map(|e| (e.rights, &e.sid)).collect();
    let assigned: BTreeSet<Sid> = entries
        .iter()
        .filter(|e| OWN_KINDS.contains(&e.kind) && e.rights == Rights::Modify)
        .map(|e| e.sid.clone())
        .collect();
    Strict {
        assigned: assigned.into_iter().collect(),
        ..strict_from(&items)
    }
}

/// `strict` for a walk of `root`: in a matter's tree (inside one of `own`, the closest), with
/// the subjects `assigned` gives that matter, so a child another pool user owns is handed back
/// (S15 `PR4h`); outside every matter's tree, as it is.
pub(super) fn strict_at(
    strict: &Strict,
    own: &[OwnRoot],
    root: &Path,
    assigned: impl FnOnce(MatterId) -> Vec<Sid>,
) -> Strict {
    let matter = own
        .iter()
        .filter(|(tree, _)| in_folder(root, tree))
        .max_by_key(|(tree, _)| tree.components().count())
        .map(|(_, matter)| *matter);
    Strict {
        assigned: matter.map(assigned).unwrap_or_default(),
        ..strict.clone()
    }
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
        assigned: Vec::new(),
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
/// names (this run wrote its entry, or its entry was still pending), the launch's own matter,
/// state and notes folders when `own` (always, unless the launch check let them go: S15
/// `PR3d`), and `extra` (the check's targeted walks). A walk inside another walked tree is
/// left to that walk. A tree is strict when it is one of the launch's own folders or carries a
/// deny.
pub(super) fn walks(
    plan: &AdmittedPlan,
    unwalked: impl Fn(usize) -> bool,
    own: bool,
    extra: &[Walk],
) -> Vec<Walk> {
    let mut found: BTreeMap<(usize, PathBuf), Walk> = BTreeMap::new();
    for (i, (grant, owner)) in plan.grants().iter().zip(plan.owners()).enumerate() {
        if grant.inherit() == Inherit::ThisOnly {
            continue;
        }
        let is_own = *owner == plan.matter()
            && grant.rights() == Rights::Modify
            && OWN_KINDS.contains(&grant.kind());
        let always = is_own && own;
        if !always && !unwalked(i) {
            continue;
        }
        let own = is_own;
        let reach = reach_of(grant.inherit());
        let key = (grant.path().components().count(), grant.path().to_owned());
        let walk = found.entry(key).or_insert_with(|| Walk {
            root: grant.path().to_owned(),
            reach,
            strict: false,
            kind: grant.kind(),
        });
        walk.strict |= own || grant.rights().is_deny();
        walk.reach = wider(walk.reach, reach);
    }
    for more in extra {
        let key = (more.root.components().count(), more.root.clone());
        let walk = found.entry(key).or_insert_with(|| more.clone());
        walk.strict |= more.strict;
        walk.reach = wider(walk.reach, more.reach);
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

/// The trees the owner pass goes over (S15 `PR4a2`), outermost first and none inside another:
/// an owner gets `WRITE_DAC` before any deny is read, so a child a pool user owns below a deny
/// could take the deny's inherited copy off.
/// - Each protected tree (a read-only path's deny in the matter, for the tree) whose entry this
///   run wrote or found still pending (`unwalked`): a crash between the deny and its pass
///   leaves it pending.
/// - On a full walk (`own`), every protected tree.
/// - On a targeted one, each touched folder (`extra`) inside a protected tree: a file the lawyer
///   moved into `.claude\commands` or `.git\hooks` between sessions; and each protected tree
///   inside a touched tree, or directly in a touched folder: a folder moved in that holds a
///   `.claude` already denied.
///
/// A skipped launch has none: nothing in the trees changed since a clean close, and no pool user
/// can add a child below a deny.
pub(super) fn owner_passes(
    plan: &AdmittedPlan,
    unwalked: impl Fn(usize) -> bool,
    own: bool,
    extra: &[Walk],
) -> Vec<PathBuf> {
    let is_tree = |g: &&crate::windows::acl_plan::Grant| {
        g.kind() == PathKind::ReadOnlyInMatter
            && g.rights() == Rights::DenyWrite
            && g.inherit() == Inherit::Tree
    };
    let trees: Vec<&Path> = plan
        .grants()
        .iter()
        .filter(is_tree)
        .map(crate::windows::acl_plan::Grant::path)
        .collect();
    let mut roots: Vec<PathBuf> = plan
        .grants()
        .iter()
        .enumerate()
        .filter(|(i, g)| is_tree(g) && (own || unwalked(*i)))
        .map(|(_, g)| g.path().to_owned())
        .collect();
    roots.extend(
        extra
            .iter()
            .map(|walk| &walk.root)
            .filter(|root| trees.iter().any(|tree| in_folder(root, tree)))
            .cloned(),
    );
    // A touched tree holds every protected tree inside it; a touched folder's children only
    // those directly in it (the parent is the folder: each in the other).
    let reaches = |tree: &Path, walk: &Walk| match walk.reach {
        Reach::Tree => in_folder(tree, &walk.root),
        Reach::Children | Reach::FilesOnly => tree
            .parent()
            .is_some_and(|p| in_folder(p, &walk.root) && in_folder(&walk.root, p)),
    };
    roots.extend(
        trees
            .iter()
            .filter(|tree| extra.iter().any(|walk| reaches(tree, walk)))
            .map(|tree| tree.to_path_buf()),
    );
    roots.sort();
    roots.dedup();
    let outermost = |root: &PathBuf| !roots.iter().any(|o| o != root && in_folder(root, o));
    roots
        .iter()
        .filter(|root| outermost(root))
        .cloned()
        .collect()
}

/// The farther of two reaches: the whole tree, then every child, then the files alone.
fn wider(a: Reach, b: Reach) -> Reach {
    let rank = |r: Reach| match r {
        Reach::FilesOnly => 0,
        Reach::Children => 1,
        Reach::Tree => 2,
    };
    if rank(b) > rank(a) { b } else { a }
}

/// How far an entry's inheritance reaches.
pub(super) fn reach_of(inherit: Inherit) -> Reach {
    match inherit {
        Inherit::FilesInFolder => Reach::FilesOnly,
        Inherit::Tree | Inherit::Below | Inherit::ThisOnly => Reach::Tree,
    }
}

/// Whether `path` is inside one of `roots` (or is one).
pub(super) fn inside_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| in_folder(path, root))
}
