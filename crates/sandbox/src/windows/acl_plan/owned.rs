//! The folders Clauseo itself makes for a session under its data folder, each given a protected
//! DACL ([`OwnedRoot`]): `SE_DACL_PROTECTED`, nothing inherited from the folder above, so a
//! data folder under `C:\` no longer hands down `C:\`'s `(A;CI;LC;;;BU)` and `(A;CI;DC;;;BU)`,
//! which let one matter's agent make files and folders in another session's state and temp
//! folders (the Windows VM's observer run, ADR 0049 "Owned roots").
//!
//! Each owned root's DACL holds, explicitly ([`OwnedRoot::fixed`]): full access (`FA`, inherited
//! by files and folders inside, `OICI`) for `SYSTEM` and the desktop user, and for
//! `Administrators` too on the data folder and its `agent-state` and `run` folders, as the
//! sandbox folder's DACL has it; on the state, notes and temp folders an inherit-only `OWNER
//! RIGHTS` allow of `Modify` (no `WRITE_DAC` or `WRITE_OWNER`), so an agent can't open the
//! files it makes to another matter's agent; then the plan's own entries and every allow the
//! ledger records there (the pool user's `Modify`, the other pool users' denies). Nothing for
//! `Users`, `Authenticated Users`, `Everyone` or `CREATOR OWNER`. The Low label is a plan
//! entry, as before. What the agent makes inside inherits these entries alone.
//!
//! Only folders inside `<data_dir>` are owned; the lawyer's matter folders never are (a separate
//! decision). A state, notes or temp folder outside the data folder is left as it is, and so is a
//! data folder that is a volume's root or holds the profile.

use std::path::{Component, Path, PathBuf};

use super::targets::{AGENT_STATE_DIR, RUN_DIR};
use super::{AdmittedPlan, Grant, Inherit, Rights, Sid, in_folder};
use crate::Policy;
use crate::windows::admit::PathKind;

/// `SYSTEM`.
const SYSTEM_SID: &str = "S-1-5-18";
/// `BUILTIN\Administrators`.
const ADMINISTRATORS_SID: &str = "S-1-5-32-544";
/// `OWNER RIGHTS`: what the owner of an item may do, in place of its implied `READ_CONTROL`
/// and `WRITE_DAC`.
const OWNER_RIGHTS_SID: &str = "S-1-3-4";
/// `FILE_ALL_ACCESS` (`FA`).
const FULL_ACCESS: u32 = 0x001f_01ff;

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

/// One folder Clauseo owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedRoot {
    pub(crate) path: PathBuf,
    pub(crate) kind: OwnedKind,
}

/// One allow every owned root of its kind gets besides the plan's ([`OwnedRoot::fixed`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedEntry {
    sid: Sid,
    mask: u32,
    inherit_only: bool,
}

impl FixedEntry {
    /// Whom.
    pub fn sid(&self) -> &Sid {
        &self.sid
    }

    /// What.
    pub fn mask(&self) -> u32 {
        self.mask
    }

    /// Whether it reaches only what is inside (`OICIIO`), not the folder itself (`OICI`).
    pub fn inherit_only(&self) -> bool {
        self.inherit_only
    }

    /// Its SDDL.
    fn sddl(&self) -> String {
        let (flags, sid) = (if self.inherit_only { "OICIIO" } else { "OICI" }, &self.sid);
        if self.mask == FULL_ACCESS {
            format!("(A;{flags};FA;;;{sid})")
        } else {
            format!("(A;{flags};0x{:x};;;{sid})", self.mask)
        }
    }
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

    /// Whether what is already inside is handed back and worked out again once its DACL is
    /// written: the session's and the matter's own folders. Not the data folder's two shared
    /// folders, which hold every other matter and session (each protected and walked at its own
    /// launch); the data folder's other contents get a walk of their own (`apply/owned.rs`).
    pub fn walked(&self) -> bool {
        !matches!(self.kind, OwnedKind::DataRoot | OwnedKind::DataFolder)
    }

    /// Whether what is inside is the agent's own (the state and notes folders, and the folder
    /// holding them): a pool user's item there goes back to the lawyer only when it isn't the
    /// matter's own. In the session's fresh run and temp folders, every pool user's item does.
    pub fn agents_own(&self) -> bool {
        matches!(
            self.kind,
            OwnedKind::MatterFolder | OwnedKind::State | OwnedKind::Notes
        )
    }

    /// Its fixed entries, in order, for the desktop user `owner`: full access for `SYSTEM`,
    /// `Administrators` on the data folder and its shared folders, and `owner`; then, on the
    /// state, notes and temp folders, `OWNER RIGHTS`' inherit-only `Modify`.
    pub fn fixed(&self, owner: &Sid) -> Vec<FixedEntry> {
        let admins = matches!(self.kind, OwnedKind::DataRoot | OwnedKind::DataFolder);
        let mut sids: Vec<Sid> = [SYSTEM_SID]
            .into_iter()
            .chain(admins.then_some(ADMINISTRATORS_SID))
            .filter_map(|s| Sid::parse(s).ok())
            .collect();
        if !sids.contains(owner) {
            sids.push(owner.clone());
        }
        let full = |sid| FixedEntry {
            sid,
            mask: FULL_ACCESS,
            inherit_only: false,
        };
        let mut out: Vec<FixedEntry> = sids.into_iter().map(full).collect();
        let agents = matches!(
            self.kind,
            OwnedKind::State | OwnedKind::Notes | OwnedKind::TempFolder
        );
        if agents && let Ok(sid) = Sid::parse(OWNER_RIGHTS_SID) {
            let mask = Rights::Modify.mask();
            out.push(FixedEntry {
                sid,
                mask,
                inherit_only: true,
            });
        }
        out
    }
}

/// The folders `policy`'s launch owns under `data` (canonical), shallowest first. Called once
/// every grant has passed `check_target`, so a state folder inside `data` is one level below
/// `<data>\agent-state\<matter>`, the notes folder is `<data>\agent-state\<matter>\notes` and
/// the temp folder `<data>\run\<session>\tmp`; each is checked again here all the same.
pub(super) fn owned_roots(policy: &Policy, data: &Path) -> Vec<OwnedRoot> {
    let mut out: Vec<OwnedRoot> = Vec::new();
    let mut push = |path: &Path, kind: OwnedKind| {
        if !out.iter().any(|r| same(&r.path, path)) {
            let path = path.to_owned();
            out.push(OwnedRoot { path, kind });
        }
    };
    if data_root_owned(policy, data) {
        push(data, OwnedKind::DataRoot);
    }
    let agent_state = data.join(AGENT_STATE_DIR);
    let mut matter_folder = |folder: &Path, kind: OwnedKind| {
        let Some(matter) = folder.parent().filter(|m| below(m, &agent_state)) else {
            return;
        };
        push(&agent_state, OwnedKind::DataFolder);
        push(matter, OwnedKind::MatterFolder);
        push(folder, kind);
    };
    matter_folder(policy.state(), OwnedKind::State);
    if let Some(notes) = policy.notes() {
        matter_folder(notes, OwnedKind::Notes);
    }
    let run = data.join(RUN_DIR);
    if let Some(tmp) = policy.session_tmpdir()
        && let Some(session) = tmp.parent().filter(|s| below(s, &run))
    {
        push(&run, OwnedKind::DataFolder);
        push(session, OwnedKind::RunFolder);
        push(tmp, OwnedKind::TempFolder);
    }
    out.sort_by_key(|r| r.path.components().count());
    out
}

/// Whether the data folder `data` itself is owned: not when it is a volume's root (no name of
/// its own: `C:\`) or is or holds the profile, whose DACLs are Windows' and the lawyer's.
fn data_root_owned(policy: &Policy, data: &Path) -> bool {
    let named = data.components().any(|c| matches!(c, Component::Normal(_)));
    let holds_profile = policy.home().is_some_and(|home| in_folder(home, data));
    named && !holds_profile
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
/// with the plan's denies on it, the fixed entries ([`OwnedRoot::fixed`]), then its allows;
/// and its label, when the plan gives one. Allows another launch recorded there stay as well.
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
    parts.extend(root.fixed(owner).iter().map(FixedEntry::sddl));
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
