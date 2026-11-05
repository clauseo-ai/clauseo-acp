//! The grant plan: every permission entry one tier B launch needs, as data. Pure: it reads only
//! what the gate admitted ([`Admitted`]) and the launch's SIDs ([`Subjects`]), and writes
//! nothing; [`apply`](super::apply::apply) applies it, through the write-ahead ledger.
//!
//! Tier B runs each matter as its own pool user (design B): the token's restricting set is {the
//! pool user, `RESTRICTED`, `Users`, `Everyone`}, at Low integrity. The rules:
//!
//! - Every allow names the matter's pool user, and also the matter's SID when the launch asks
//!   for it ([`Subjects::matter`], off by default; spike B found it adds nothing measured).
//! - `Modify` on the matter, the state and notes folders and the credential link;
//!   `ReadAndExecute` on runtime and read paths and allowed folders; on Clauseo's Claude login
//!   home, `CreateFiles` on the folder and `Modify` on its allowed names.
//! - A Low mandatory label, `S:(ML;OICI;NW;;;LW)` (no-write-up), inherited on the matter, state
//!   and notes folders and the Claude store folder, and on the credential link file itself
//!   ([`Rights::LowLabel`]): the Low token writes nothing labelled higher. Side effect, accepted:
//!   the lawyer's own Low-integrity processes can write those too. Whether the Claude store
//!   takes writes at Low is PR6's VM row (the store and credential rows of done-when 2).
//! - A deny of writes for the pool user on each of the policy's read-only paths in the matter and
//!   the state folder: one deny refuses both of the token's checks.
//! - Isolation between matters: the fence ([`plan_fence`]) of this matter, an inherited deny of
//!   reads and writes for every other pool user on its matter, state and notes folders, so a
//!   matter that inherits `C:\`'s `Users` write entries is closed to other matters' agents
//!   (spike B, rows 2 and 4); and the fence of every registered matter not fenced yet, each
//!   recorded under that matter's id ([`AdmittedPlan::owners`]).
//! - Sibling folders: on each folder beside the matter in its parent that the gate listed
//!   ([`Admitted::siblings`]), an inherited deny of reads for the `ClauseoSandboxUsers` group,
//!   which holds every pool user; and one more on the parent for the files directly in it
//!   ([`Inherit::FilesInFolder`]). The parent's own listing still shows the names (a known
//!   limit).
//! - Nothing in the lawyer's `<profile>\.claude`, in any letter case: NTFS can't limit a create
//!   right by name, and the lawyer's own Claude Code loads what is there. A policy that would
//!   grant there is refused.
//! - No entry on the profile folder or above it, none on, over or in `<data_dir>\sandbox\`
//!   (the SID map, the ledger, the marker); inside `<data_dir>\agents\` only read and run
//!   grants, outside agents' homes, besides the Claude store and credential link themselves; and
//!   no read, runtime or allowed path elsewhere over or in `<data_dir>` outside this matter's
//!   own state and notes folders. Such a plan is refused (see [`check_target`]).
//!
//! The plan comes back as an [`AdmittedPlan`]: only [`plan`] makes one, from an [`Admitted`]
//! only the gate makes, and only an `AdmittedPlan` reaches the ledger.

use std::path::{Component, Path, PathBuf};

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};

use super::admit::{Admitted, PathKind};
#[cfg(any(windows, test, feature = "test-support"))]
use super::fence::Fence;
use super::fence::{MatterFolders, fence_grants, plan_fence};
use super::ledger::EntryId;
pub use super::sids::Sid;
use crate::policy::under;
use crate::{Error, PathRule, Policy};

/// The SIDs a launch's entries name. PR4 makes the pool and the group; the plan takes them as
/// values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subjects {
    /// This matter's pool user: every allow and the read-only denies name it.
    pub pool_user: Sid,
    /// This matter's own SID as a second subject of every allow, when set. Off by default.
    pub matter: Option<Sid>,
    /// Every other pool user: each is denied this matter's matter, state and notes folders.
    pub other_pool_users: Vec<Sid>,
    /// The `ClauseoSandboxUsers` group, holding every pool user: denied reading the siblings.
    pub pool_group: Sid,
}

/// `OWNER RIGHTS`, `S-1-3-4`: an allow for it replaces the rights a file's owner gets
/// implicitly (`READ_CONTROL` and `WRITE_DAC`).
pub const OWNER_RIGHTS_SID: &str = "S-1-3-4";

/// The Low mandatory level, `S-1-16-4096` (`LW`): the subject of a [`Rights::LowLabel`] item.
pub const LOW_LABEL_SID: &str = "S-1-16-4096";

/// The rights an entry gives or denies. Stored in the ledger by its snake-case name; new rights
/// are appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rights {
    /// Allow `Modify` (`0x1301bf`): read, write, run and delete, without `FILE_DELETE_CHILD`,
    /// `WRITE_DAC` or `WRITE_OWNER`, so the agent can't strip a deny or delete a child through
    /// its parent.
    Modify,
    /// Allow `ReadAndExecute` (`0x1200a9`).
    ReadAndExecute,
    /// Allow `CreateFiles` (`FILE_ADD_FILE`, `0x2`) on a folder: make a file in it, nothing more.
    CreateFiles,
    /// Deny writing (`0xd0156`): data, append, attributes, extended attributes, `DELETE`,
    /// `FILE_DELETE_CHILD`, `WRITE_DAC` and `WRITE_OWNER`.
    DenyWrite,
    /// Deny reading and writing (`0xf01ff`): [`Rights::DenyWrite`] plus read data, read
    /// attributes, read extended attributes, execute and `READ_CONTROL`.
    DenyReadWrite,
    /// Deny reading (`0x200a9`): read data, read attributes, read extended attributes, execute
    /// and `READ_CONTROL`.
    DenyRead,
    /// Not an access entry: the Low mandatory label `S:(ML;OICI;NW;;;LW)` (mask `0x1`,
    /// no-write-up), set with `LABEL_SECURITY_INFORMATION`; its subject is [`LOW_LABEL_SID`].
    LowLabel,
    /// Deny changing permissions and owner (`WRITE_DAC | WRITE_OWNER`, `0xc0000`): a file the
    /// agent makes under a folder with an inheritable `CREATOR OWNER` allow (such as `C:\`'s
    /// full control) would otherwise give the agent `WRITE_DAC` on it.
    DenyChangePermissions,
}

impl Rights {
    /// The access mask.
    pub fn mask(self) -> u32 {
        match self {
            Rights::Modify => 0x0013_01bf,
            Rights::ReadAndExecute => 0x0012_00a9,
            Rights::CreateFiles => 0x0000_0002,
            Rights::DenyWrite => DENY_WRITE,
            Rights::DenyReadWrite => DENY_WRITE | DENY_READ,
            Rights::DenyRead => DENY_READ,
            Rights::LowLabel => 0x1,
            Rights::DenyChangePermissions => 0x000c_0000,
        }
    }

    /// Whether the entry is a deny entry.
    pub fn is_deny(self) -> bool {
        matches!(
            self,
            Rights::DenyWrite
                | Rights::DenyReadWrite
                | Rights::DenyRead
                | Rights::DenyChangePermissions
        )
    }

    /// Whether the item is the mandatory label rather than an access entry.
    pub fn is_label(self) -> bool {
        self == Rights::LowLabel
    }

    /// Whether the entry allows access.
    pub fn is_allow(self) -> bool {
        !self.is_deny() && !self.is_label()
    }
}

/// `FILE_WRITE_DATA | FILE_APPEND_DATA | FILE_WRITE_EA | FILE_DELETE_CHILD |
/// FILE_WRITE_ATTRIBUTES | DELETE | WRITE_DAC | WRITE_OWNER`.
const DENY_WRITE: u32 = 0x2 | 0x4 | 0x10 | 0x40 | 0x100 | 0x0001_0000 | 0x0004_0000 | 0x0008_0000;

/// `FILE_READ_DATA | FILE_READ_EA | FILE_EXECUTE | FILE_READ_ATTRIBUTES | READ_CONTROL`.
const DENY_READ: u32 = 0x1 | 0x8 | 0x20 | 0x80 | 0x0002_0000;

/// Where an entry reaches: the path alone, or the path and everything under it (object and
/// container inherit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Inherit {
    /// The path alone.
    ThisOnly,
    /// The path and everything under it.
    Tree,
    /// Only the files directly in the folder: object inherit, inherit only, no propagation. The
    /// folder itself and every folder in it are untouched.
    FilesInFolder,
}

/// One permission entry to write. Made only inside this crate: by [`plan`], or read back from
/// the ledger.
///
/// Written by `NtfsAcl` (`acl.rs`): every target opened with `FILE_FLAG_OPEN_REPARSE_POINT` and
/// the entry set through that handle, a junction or symbolic link or a changed final path
/// refused, as defence in depth: the gate checked the names the policy keeps unresolved (the
/// credential link, the Claude store's files) and the agents' folders, but a name can change
/// between the gate and the write.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Grant {
    pub(crate) path: PathBuf,
    pub(crate) sid: Sid,
    pub(crate) rights: Rights,
    pub(crate) inherit: Inherit,
    pub(crate) kind: PathKind,
}

impl Grant {
    /// Where, canonical as the policy's paths are.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whom.
    pub fn sid(&self) -> &Sid {
        &self.sid
    }

    /// What.
    pub fn rights(&self) -> Rights {
        self.rights
    }

    /// How far down.
    pub fn inherit(&self) -> Inherit {
        self.inherit
    }

    /// Why: which of the policy's paths this is.
    pub fn kind(&self) -> PathKind {
        self.kind
    }
}

/// A launch's plan, made only by [`plan`] from what the gate admitted (or, when the pool grows,
/// from fences the gate's check resolved, `from_fences`): the one thing the ledger records
/// ([`Ledger::record_pending`](super::ledger::Ledger::record_pending)). Once recorded it
/// also holds the entry numbers the ledger gave its grants, and which of them this launch made,
/// so a rollback drops only its own.
#[derive(Debug)]
pub struct AdmittedPlan {
    matter: MatterId,
    owners: Vec<MatterId>,
    grants: Vec<Grant>,
    pub(crate) entries: Vec<EntryId>,
    pub(crate) created: Vec<EntryId>,
}

impl AdmittedPlan {
    /// The matter this plan is for.
    pub fn matter(&self) -> MatterId {
        self.matter
    }

    /// The grants, in order.
    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    /// The matter whose ledger entry each grant is, in the grants' order: this launch's matter,
    /// or a registered matter whose fence this launch plans.
    pub fn owners(&self) -> &[MatterId] {
        &self.owners
    }

    /// Each grant's ledger entry, in the grants' order, once recorded; empty before.
    pub fn entries(&self) -> &[EntryId] {
        &self.entries
    }

    /// The entries this launch added to the ledger (not ones an earlier launch listed), less any
    /// it has dropped since.
    pub fn created(&self) -> &[EntryId] {
        &self.created
    }

    /// Leaves out each grant whose path `exists` says isn't there, before the plan is recorded
    /// (after, it changes nothing): NTFS holds no entry for a missing name.
    pub(crate) fn retain_existing(&mut self, mut exists: impl FnMut(&Path) -> bool) {
        if !self.entries.is_empty() {
            return;
        }
        let (grants, owners) = std::mem::take(&mut self.grants)
            .into_iter()
            .zip(std::mem::take(&mut self.owners))
            .filter(|(grant, _)| exists(&grant.path))
            .unzip();
        self.grants = grants;
        self.owners = owners;
    }

    /// The plan of `fences`, as the gate's check resolved them, against the whole `pool`, each
    /// grant owned by its matter: what [`fence_registered`](super::apply::fence_registered)
    /// applies when the pool grows. `None` when there is no fence.
    #[cfg(any(windows, test, feature = "test-support"))]
    pub(crate) fn from_fences(fences: &[Fence], pool: &[Sid]) -> Option<AdmittedPlan> {
        let matter = fences.first()?.id;
        let (mut grants, mut owners) = (Vec::new(), Vec::new());
        for fence in fences {
            let folders: Vec<(&Path, PathKind)> = fence
                .folders
                .iter()
                .map(|(p, k)| (p.as_path(), *k))
                .collect();
            for grant in fence_grants(&folders, &fence.pool_user, pool) {
                grants.push(grant);
                owners.push(fence.id);
            }
        }
        Some(AdmittedPlan {
            matter,
            owners,
            grants,
            entries: Vec::new(),
            created: Vec::new(),
        })
    }

    /// A plan of `grants` without the gate, for the ledger's tests.
    #[cfg(test)]
    pub(crate) fn from_grants(matter: MatterId, grants: Vec<Grant>) -> AdmittedPlan {
        AdmittedPlan {
            matter,
            owners: vec![matter; grants.len()],
            grants,
            entries: Vec::new(),
            created: Vec::new(),
        }
    }
}

/// The plan for `admitted`'s policy with `subjects`, in a fixed order: the writable folders,
/// the credential link, the Claude store, the read paths, the Low labels, the read-only denies,
/// the other pool users' denies, then the siblings' denies. Each entry appears once.
///
/// Refuses ([`Error::Policy`], naming the kind of path but not the path) when the subjects
/// collide (the pool user is the group, another pool user or the matter SID), or when any entry
/// would land in `<home>\.claude`; and refuses ([`Error::FolderUnsupported`]) an entry the
/// profile or data rules forbid (see [`check_target`]).
pub fn plan(
    admitted: &Admitted<'_>,
    subjects: &Subjects,
    matter: MatterId,
) -> Result<AdmittedPlan, Error> {
    let pool = &subjects.pool_user;
    if *pool == subjects.pool_group
        || subjects.other_pool_users.contains(pool)
        || subjects.matter.as_ref() == Some(pool)
    {
        return Err(Error::Policy(
            "the pool user's SID is also another subject's".to_owned(),
        ));
    }
    let policy = admitted.policy();
    let mut plan = Plan {
        subjects,
        grants: Vec::new(),
    };
    let mut own = vec![
        (policy.matter(), PathKind::Matter),
        (policy.state(), PathKind::State),
    ];
    if let Some(notes) = policy.notes() {
        own.push((notes, PathKind::Notes));
    }
    for &(folder, kind) in &own {
        plan.allow(folder, Rights::Modify, Inherit::Tree, kind);
    }
    if let Some(link) = policy.credential_link() {
        let kind = PathKind::CredentialLink;
        plan.allow(link, Rights::Modify, Inherit::ThisOnly, kind);
    }
    if let Some(store) = policy.claude_store() {
        let (dir, kind) = (store.dir(), PathKind::ClaudeStore);
        plan.allow(dir, Rights::CreateFiles, Inherit::ThisOnly, kind);
        for file in store.files() {
            let kind = PathKind::ClaudeStoreFile;
            plan.allow(&file, Rights::Modify, Inherit::ThisOnly, kind);
        }
    }
    let read_paths = [
        (policy.runtime(), PathKind::Runtime),
        (policy.read(), PathKind::Read),
        (policy.allowed_folders(), PathKind::AllowedFolder),
    ];
    for (paths, kind) in read_paths {
        for path in paths {
            plan.allow(path, Rights::ReadAndExecute, Inherit::Tree, kind);
        }
    }
    plan.owner_rights(policy, &own)?;
    plan.labels(policy, &own)?;
    plan.read_only(policy);
    let mut pool = vec![pool.clone()];
    pool.extend(subjects.other_pool_users.iter().cloned());
    let folders = MatterFolders {
        root: policy.matter().to_owned(),
        state: policy.state().to_owned(),
        notes: policy.notes().map(Path::to_owned),
    };
    for grant in plan_fence(&folders, &subjects.pool_user, &pool) {
        plan.push_grant(grant);
    }
    for sibling in admitted.siblings() {
        let (group, kind) = (subjects.pool_group.clone(), PathKind::Sibling);
        plan.push(sibling, group, Rights::DenyRead, Inherit::Tree, kind);
    }
    if let Some(parent) = admitted.parent_files() {
        let (group, kind) = (subjects.pool_group.clone(), PathKind::ParentFiles);
        plan.push(
            parent,
            group,
            Rights::DenyRead,
            Inherit::FilesInFolder,
            kind,
        );
    }
    // Every registered matter whose fence the ledger doesn't list as applied, as the gate
    // checked it: its entries are its own (the ledger records them under its id).
    let mut owners = vec![matter; plan.grants.len()];
    for fence in admitted.fences() {
        let before = plan.grants.len();
        let folders: Vec<(&Path, PathKind)> = fence
            .folders
            .iter()
            .map(|(p, k)| (p.as_path(), *k))
            .collect();
        for grant in fence_grants(&folders, &fence.pool_user, &pool) {
            plan.push_grant(grant);
        }
        owners.resize(owners.len() + plan.grants.len() - before, fence.id);
    }
    if let Some(home) = policy.home() {
        let claude = home.join(".claude");
        if let Some(grant) = plan.grants.iter().find(|g| in_folder(&g.path, &claude)) {
            return Err(Error::Policy(format!(
                "tier B grants nothing in the profile's .claude folder, and {} is in it",
                grant.kind
            )));
        }
    }
    for grant in &plan.grants {
        if let Some(refusal) = check_target(admitted, grant, matter) {
            return Err(Error::FolderUnsupported {
                path_kind: grant.kind,
                support: refusal,
            });
        }
    }
    Ok(AdmittedPlan {
        matter,
        owners,
        grants: plan.grants,
        entries: Vec::new(),
        created: Vec::new(),
    })
}

/// The entries planned so far.
struct Plan<'s> {
    subjects: &'s Subjects,
    grants: Vec<Grant>,
}

impl Plan<'_> {
    /// The Low labels: inherited on this matter's own folders and the Claude store folder, and
    /// on the credential link file itself. The Low token writes no object labelled above Low,
    /// so the store and the link the agent writes get it too; the side effect, accepted, is that
    /// the lawyer's own Low processes can write them as well. The credential link is a hard link
    /// by design, and a hard link shares its file's security descriptor: labelling it relabels
    /// the lawyer's own credential file (PR6's VM row checks it).
    fn labels(&mut self, policy: &Policy, own: &[(&Path, PathKind)]) -> Result<(), Error> {
        let low = Sid::parse(LOW_LABEL_SID).map_err(|error| Error::Policy(error.to_string()))?;
        for &(folder, kind) in own {
            self.push(folder, low.clone(), Rights::LowLabel, Inherit::Tree, kind);
        }
        if let Some(store) = policy.claude_store() {
            let kind = PathKind::ClaudeStore;
            self.push(
                store.dir(),
                low.clone(),
                Rights::LowLabel,
                Inherit::Tree,
                kind,
            );
        }
        if let Some(link) = policy.credential_link() {
            let kind = PathKind::CredentialLink;
            self.push(link, low, Rights::LowLabel, Inherit::ThisOnly, kind);
        }
        Ok(())
    }

    /// What an agent owns, it may not re-permission. On the matter, state and notes folders, an
    /// inherited allow for `OWNER RIGHTS` with the pool user's own `Modify`, which replaces the
    /// owner's implicit `READ_CONTROL | WRITE_DAC`; on the Claude store folder, the same for the
    /// files in it (the rights the pool user has on the store's files). With each, the pool
    /// user's inherited deny of `WRITE_DAC | WRITE_OWNER`, which a `CREATOR OWNER` allow
    /// inherited from above can't beat (the deny comes first in the copy a new file gets). The
    /// lawyer keeps full control of the lawyer's own files: their allows for the lawyer's SID
    /// (explicit or inherited) still apply; only the implicit owner rights are replaced.
    fn owner_rights(&mut self, policy: &Policy, own: &[(&Path, PathKind)]) -> Result<(), Error> {
        let owner = Sid::parse(OWNER_RIGHTS_SID).map_err(|e| Error::Policy(e.to_string()))?;
        let pool = self.subjects.pool_user.clone();
        let mut targets: Vec<(&Path, PathKind, Inherit)> =
            own.iter().map(|&(p, k)| (p, k, Inherit::Tree)).collect();
        if let Some(store) = policy.claude_store() {
            targets.push((store.dir(), PathKind::ClaudeStore, Inherit::FilesInFolder));
        }
        for (path, kind, inherit) in targets {
            self.push(path, owner.clone(), Rights::Modify, inherit, kind);
            let deny = Rights::DenyChangePermissions;
            self.push(path, pool.clone(), deny, Inherit::Tree, kind);
        }
        Ok(())
    }

    /// The pool user's deny of writes on each read-only path in the matter and state folder.
    fn read_only(&mut self, policy: &Policy) {
        let pool = self.subjects.pool_user.clone();
        for rule in policy.read_only_in_matter() {
            let inherit = match rule {
                PathRule::Literal(_) => Inherit::ThisOnly,
                PathRule::Subpath(_) => Inherit::Tree,
            };
            let kind = PathKind::ReadOnlyInMatter;
            self.push(rule.path(), pool.clone(), Rights::DenyWrite, inherit, kind);
        }
        for path in policy.read_only_in_state() {
            let kind = PathKind::ReadOnlyInState;
            self.push(path, pool.clone(), Rights::DenyWrite, Inherit::Tree, kind);
        }
    }

    /// An allow for the pool user, and the same for the matter's SID when it is a subject.
    fn allow(&mut self, path: &Path, rights: Rights, inherit: Inherit, kind: PathKind) {
        let pool = self.subjects.pool_user.clone();
        self.push(path, pool, rights, inherit, kind);
        if let Some(matter) = self.subjects.matter.clone() {
            self.push(path, matter, rights, inherit, kind);
        }
    }

    fn push(&mut self, path: &Path, sid: Sid, rights: Rights, inherit: Inherit, kind: PathKind) {
        self.push_grant(Grant {
            path: path.to_owned(),
            sid,
            rights,
            inherit,
            kind,
        });
    }

    fn push_grant(&mut self, grant: Grant) {
        let same = |g: &Grant| {
            g.path == grant.path
                && g.sid == grant.sid
                && g.rights == grant.rights
                && g.inherit == grant.inherit
        };
        if !self.grants.iter().any(same) {
            self.grants.push(grant);
        }
    }
}

/// Whether `path` is `folder` or under it, by the policy's comparison and also without ASCII
/// case, as NTFS compares `.claude` with `.Claude`.
pub(super) fn in_folder(path: &Path, folder: &Path) -> bool {
    if under(path, folder) {
        return true;
    }
    let mut parts = path.components();
    folder.components().all(|want| {
        parts.next().is_some_and(|got| match (got, want) {
            (Component::Normal(got), Component::Normal(want)) => got.eq_ignore_ascii_case(want),
            (Component::Prefix(got), Component::Prefix(want)) => {
                got.as_os_str().eq_ignore_ascii_case(want.as_os_str())
            }
            (got, want) => got == want,
        })
    })
}

pub(super) mod targets;

use targets::check_target;
#[cfg(test)]
use targets::odd_spelling;

#[cfg(test)]
mod tests;
