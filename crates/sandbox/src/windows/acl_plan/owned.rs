//! The folders Clauseo itself makes for a session under its data folder, each given a protected
//! DACL ([`OwnedRoot`]): `SE_DACL_PROTECTED`, nothing inherited from the folder above, so a
//! data folder under `C:\` no longer hands down `C:\`'s `(A;CI;LC;;;BU)` and `(A;CI;DC;;;BU)`,
//! which let one matter's agent make files and folders in another session's state and temp
//! folders (the Windows VM's observer run, ADR 0049 "Owned roots").
//!
//! Each owned root's DACL holds, explicitly: full access (`FA`, inherited by files and folders
//! inside, `OICI`) for `SYSTEM` and the desktop user, and for `Administrators` too on the data
//! folder and its `agent-state` and `run` folders, as the sandbox folder's DACL has it; then this
//! plan's own entries on it, written as they are today (the pool user's `Modify` and the other
//! pool users' denies on the state and notes folders, the pool user's `Modify` on the temp
//! folder). Nothing for `Users`, `Authenticated Users`, `Everyone` or `CREATOR OWNER`. The Low
//! label is a plan entry, as before. What the agent makes inside inherits these entries alone.
//!
//! Only folders inside `<data_dir>` are owned; the lawyer's matter folders never are (a separate
//! decision). A state, notes or temp folder outside the data folder is left as it is.

use std::path::{Component, Path, PathBuf};

use super::targets::{AGENT_STATE_DIR, RUN_DIR};
use super::{AdmittedPlan, Grant, Inherit, Sid, in_folder};
use crate::Policy;
use crate::windows::admit::PathKind;

/// `SYSTEM`.
pub(crate) const SYSTEM_SID: &str = "S-1-5-18";
/// `BUILTIN\Administrators`.
pub(crate) const ADMINISTRATORS_SID: &str = "S-1-5-32-544";
/// `FILE_ALL_ACCESS` (`FA`).
#[cfg(windows)]
pub(crate) const FULL_ACCESS: u32 = 0x001f_01ff;

/// Which folder Clauseo owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnedKind {
    /// `<data_dir>` itself.
    DataRoot,
    /// `<data_dir>\agent-state` or `<data_dir>\run`: every matter's or session's folders.
    DataFolder,
    /// `<data_dir>\agent-state\<matter>`: the matter's state and notes folders.
    MatterFolder,
    /// The state folder, `<data_dir>\agent-state\<matter>\<agent>`.
    State,
    /// The notes folder, `<data_dir>\agent-state\<matter>\notes`.
    Notes,
    /// The session's run folder, `<data_dir>\run\<session>`.
    RunFolder,
    /// The session's temp folder, `<data_dir>\run\<session>\tmp`.
    TempFolder,
}

/// One folder Clauseo owns, and the subjects whose explicit allows on it are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedRoot {
    pub(crate) path: PathBuf,
    pub(crate) kind: OwnedKind,
    /// The launch's own subjects (its pool user, and its matter's SID when asked) on the state,
    /// notes and temp folders; none elsewhere.
    pub(crate) allow: Vec<Sid>,
}

impl OwnedRoot {
    /// Where, canonical as the policy's paths are.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Which folder it is.
    pub fn kind(&self) -> OwnedKind {
        self.kind
    }

    /// Whose explicit allows on it stay: the launch's subjects, on its own folders only.
    pub fn allow(&self) -> &[Sid] {
        &self.allow
    }

    /// The kind of path a refusal names.
    pub fn path_kind(&self) -> PathKind {
        match self.kind {
            OwnedKind::Notes => PathKind::Notes,
            OwnedKind::RunFolder | OwnedKind::TempFolder => PathKind::TempFolder,
            OwnedKind::DataRoot
            | OwnedKind::DataFolder
            | OwnedKind::MatterFolder
            | OwnedKind::State => PathKind::State,
        }
    }

    /// Whether what is already inside is worked out again once its DACL is written: the
    /// session's and the matter's own folders. Not the data folder or its two shared folders,
    /// which hold every other matter and session: each of those gets its own walk at its
    /// launch, and what is made in them from now on inherits the protected entries.
    pub fn walked(&self) -> bool {
        !matches!(self.kind, OwnedKind::DataRoot | OwnedKind::DataFolder)
    }

    /// The SIDs given full access, in order: `SYSTEM`, `Administrators` on the data folder and
    /// its shared folders, then `owner` (the desktop user).
    pub fn full_access(&self, owner: &Sid) -> Vec<Sid> {
        let mut out: Vec<Sid> = [SYSTEM_SID]
            .into_iter()
            .chain(self.admins().then_some(ADMINISTRATORS_SID))
            .filter_map(|s| Sid::parse(s).ok())
            .collect();
        if !out.contains(owner) {
            out.push(owner.clone());
        }
        out
    }

    fn admins(&self) -> bool {
        matches!(self.kind, OwnedKind::DataRoot | OwnedKind::DataFolder)
    }
}

/// The folders `policy`'s launch owns under `data` (canonical), shallowest first. Called once
/// every grant has passed `check_target`, so a state folder inside `data` is one level below
/// `<data>\agent-state\<matter>`, the notes folder is `<data>\agent-state\<matter>\notes` and
/// the temp folder `<data>\run\<session>\tmp`; each is checked again here all the same.
/// `subjects` are the launch's own, kept on its state, notes and temp folders.
pub(super) fn owned_roots(policy: &Policy, data: &Path, subjects: &[Sid]) -> Vec<OwnedRoot> {
    let mut out: Vec<OwnedRoot> = Vec::new();
    let mut push = |path: &Path, kind: OwnedKind, own: bool| {
        if !out.iter().any(|r| same(&r.path, path)) {
            let allow = if own { subjects.to_vec() } else { Vec::new() };
            let path = path.to_owned();
            out.push(OwnedRoot { path, kind, allow });
        }
    };
    let holds_profile = policy.home().is_some_and(|home| in_folder(home, data));
    let named = data.components().any(|c| matches!(c, Component::Normal(_)));
    if named && !holds_profile {
        push(data, OwnedKind::DataRoot, false);
    }
    let agent_state = data.join(AGENT_STATE_DIR);
    let mut matter_folder = |folder: &Path, kind: OwnedKind| {
        let Some(matter) = folder.parent().filter(|m| below(m, &agent_state)) else {
            return;
        };
        push(&agent_state, OwnedKind::DataFolder, false);
        push(matter, OwnedKind::MatterFolder, false);
        push(folder, kind, true);
    };
    matter_folder(policy.state(), OwnedKind::State);
    if let Some(notes) = policy.notes() {
        matter_folder(notes, OwnedKind::Notes);
    }
    let run = data.join(RUN_DIR);
    if let Some(tmp) = policy.session_tmpdir()
        && let Some(session) = tmp.parent().filter(|s| below(s, &run))
    {
        push(&run, OwnedKind::DataFolder, false);
        push(session, OwnedKind::RunFolder, false);
        push(tmp, OwnedKind::TempFolder, true);
    }
    out.sort_by_key(|r| r.path.components().count());
    out
}

/// Whether `path` is directly in `folder`.
fn below(path: &Path, folder: &Path) -> bool {
    path.parent().is_some_and(|parent| same(parent, folder))
}

/// Whether `a` and `b` are the same path, by [`in_folder`]'s comparison.
fn same(a: &Path, b: &Path) -> bool {
    in_folder(a, b) && in_folder(b, a)
}

/// The security descriptor `root` (one of [`AdmittedPlan::owned_roots`]) has once `plan` is
/// applied by the desktop user `owner`, as SDDL: the protected DACL (`P`, auto-inherited `AI`)
/// with the plan's denies on it, the full-access entries ([`OwnedRoot::full_access`]), then its
/// allows; and its label, when the plan gives one.
pub fn owned_sddl(plan: &AdmittedPlan, root: &OwnedRoot, owner: &Sid) -> String {
    let on: Vec<&Grant> = plan
        .grants
        .iter()
        .filter(|g| same(&g.path, &root.path))
        .collect();
    let mut parts = vec!["D:PAI".to_owned()];
    parts.extend(
        on.iter()
            .filter(|g| g.rights.is_deny())
            .map(|g| ace("D", g)),
    );
    let full = root.full_access(owner);
    parts.extend(full.iter().map(|sid| format!("(A;OICI;FA;;;{sid})")));
    parts.extend(
        on.iter()
            .filter(|g| g.rights.is_allow())
            .map(|g| ace("A", g)),
    );
    let label = on.iter().find(|g| g.rights.is_label());
    parts.extend(label.map(|g| format!("S:(ML;{};NW;;;LW)", flags(g.inherit))));
    parts.concat()
}

/// `grant`'s entry as SDDL, of type `kind`.
fn ace(kind: &str, grant: &Grant) -> String {
    let (flags, mask) = (flags(grant.inherit), grant.rights.mask());
    format!("({kind};{flags};0x{mask:x};;;{})", grant.sid)
}

/// The SDDL inheritance flags of `inherit` on a folder.
fn flags(inherit: Inherit) -> &'static str {
    match inherit {
        Inherit::ThisOnly => "",
        Inherit::Tree => "OICI",
        Inherit::FilesInFolder => "OIIONP",
        Inherit::Below => "OICIIO",
    }
}
