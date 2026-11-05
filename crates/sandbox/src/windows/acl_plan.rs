//! The grant plan: every permission entry one tier B launch needs, as data. Pure: it reads only
//! what the gate admitted ([`Admitted`]) and the two SIDs, and writes nothing; S15's PR3 applies
//! it, through the write-ahead ledger.
//!
//! The rules (S15 brief, "Grants come in pairs" and "Read-only entries"; D8):
//!
//! - Every allow comes in a pair, one entry for the `ClauseoSandbox` user's SID (the token's
//!   normal check) and one for the matter's SID (its restricted check), so matter B's token fails
//!   on matter A's folders.
//! - `Modify` on the matter, the state and notes folders and the credential link;
//!   `ReadAndExecute` on runtime and read paths and allowed folders; on Clauseo's Claude login
//!   home, `CreateFiles` on the folder and `Modify` on its allowed names.
//! - A deny of writes for the matter's SID alone on each of the policy's read-only paths in the
//!   matter and the state folder: one deny is enough, since a restricted token needs both checks
//!   to pass, and no deny ever names the user, which every matter shares.
//! - A deny of reads and writes for the matter's SID alone on each other matter's folder the
//!   gate closed (D8): `BUILTIN\Users` stays a restricting SID, so without it this matter's token
//!   passes both checks on any folder `Users` can reach.
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

use super::admit::{Admitted, FolderRefusal, PathKind};
use super::ledger::EntryId;
use super::matters::SANDBOX_DIR;
pub use super::sids::Sid;
use crate::policy::under;
use crate::{Error, PathRule};

/// The two SIDs a launch's pairs name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sids {
    /// The `ClauseoSandbox` user's SID, shared by every matter.
    pub user: Sid,
    /// This matter's own SID, a restricting SID of its token.
    pub matter: Sid,
}

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
        }
    }

    /// Whether the entry is a deny entry.
    pub fn is_deny(self) -> bool {
        matches!(self, Rights::DenyWrite | Rights::DenyReadWrite)
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
}

/// One permission entry to write. Made only inside this crate: by [`plan`], or read back from
/// the ledger.
///
/// **For PR3, which writes them:** open every target with `FILE_FLAG_OPEN_REPARSE_POINT` and
/// set the entry through that handle, refusing a reparse point that isn't the target itself, as
/// defence in depth: the gate checked the names the policy keeps unresolved (the credential link,
/// the Claude store's files) and the agents' folders, but a name can change between the gate and
/// the write.
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

/// A launch's plan, made only by [`plan`] from what the gate admitted: the one thing the ledger
/// records ([`Ledger::record_pending`](super::ledger::Ledger::record_pending)). Once recorded it
/// also holds the entry numbers the ledger gave its grants, and which of them this launch made,
/// so a rollback drops only its own.
#[derive(Debug)]
pub struct AdmittedPlan {
    matter: MatterId,
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

    /// Each grant's ledger entry, in the grants' order, once recorded; empty before.
    pub fn entries(&self) -> &[EntryId] {
        &self.entries
    }

    /// The entries this launch added to the ledger (not ones an earlier launch listed), less any
    /// it has dropped since.
    pub fn created(&self) -> &[EntryId] {
        &self.created
    }

    /// A plan of `grants` without the gate, for the ledger's tests.
    #[cfg(test)]
    pub(crate) fn from_grants(matter: MatterId, grants: Vec<Grant>) -> AdmittedPlan {
        AdmittedPlan {
            matter,
            grants,
            entries: Vec::new(),
            created: Vec::new(),
        }
    }
}

/// The plan for `admitted`'s policy with `sids`, in a fixed order: the writable folders, the
/// credential link, the Claude store, the read paths, the read-only denies, then D8's denies.
/// Each entry appears once.
///
/// Refuses ([`Error::Policy`], naming the kind of path but not the path) when the two SIDs are
/// the same, which would collapse every pair, or when any entry would land in `<home>\.claude`;
/// and refuses ([`Error::FolderUnsupported`]) an entry the profile or data rules forbid (see
/// [`check_target`]).
pub fn plan(admitted: &Admitted<'_>, sids: &Sids, matter: MatterId) -> Result<AdmittedPlan, Error> {
    if sids.user == sids.matter {
        return Err(Error::Policy(
            "the matter's SID is the sandbox user's SID".to_owned(),
        ));
    }
    let policy = admitted.policy();
    let mut plan = Plan {
        sids,
        grants: Vec::new(),
    };
    plan.pair(
        policy.matter(),
        Rights::Modify,
        Inherit::Tree,
        PathKind::Matter,
    );
    plan.pair(
        policy.state(),
        Rights::Modify,
        Inherit::Tree,
        PathKind::State,
    );
    if let Some(notes) = policy.notes() {
        plan.pair(notes, Rights::Modify, Inherit::Tree, PathKind::Notes);
    }
    if let Some(link) = policy.credential_link() {
        plan.pair(
            link,
            Rights::Modify,
            Inherit::ThisOnly,
            PathKind::CredentialLink,
        );
    }
    if let Some(store) = policy.claude_store() {
        let (dir, kind) = (store.dir(), PathKind::ClaudeStore);
        plan.pair(dir, Rights::CreateFiles, Inherit::ThisOnly, kind);
        for file in store.files() {
            let kind = PathKind::ClaudeStoreFile;
            plan.pair(&file, Rights::Modify, Inherit::ThisOnly, kind);
        }
    }
    let read_paths = [
        (policy.runtime(), PathKind::Runtime),
        (policy.read(), PathKind::Read),
        (policy.allowed_folders(), PathKind::AllowedFolder),
    ];
    for (paths, kind) in read_paths {
        for path in paths {
            plan.pair(path, Rights::ReadAndExecute, Inherit::Tree, kind);
        }
    }
    for rule in policy.read_only_in_matter() {
        let inherit = match rule {
            PathRule::Literal(_) => Inherit::ThisOnly,
            PathRule::Subpath(_) => Inherit::Tree,
        };
        plan.matter_only(
            rule.path(),
            Rights::DenyWrite,
            inherit,
            PathKind::ReadOnlyInMatter,
        );
    }
    for path in policy.read_only_in_state() {
        let kind = PathKind::ReadOnlyInState;
        plan.matter_only(path, Rights::DenyWrite, Inherit::Tree, kind);
    }
    for folder in admitted.closed() {
        let kind = PathKind::OtherMatter;
        plan.matter_only(folder, Rights::DenyReadWrite, Inherit::Tree, kind);
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
        grants: plan.grants,
        entries: Vec::new(),
        created: Vec::new(),
    })
}

/// Why `grant` may not be written, if it may not. `<data_dir>` is an allow list:
///
/// - any path with a name that ends in `.` or a space, or holds `:` (an NTFS stream, such as
///   `home::$INDEX_ALLOCATION`), is refused first ([`FolderRefusal::OddSpelling`]): Windows reads
///   each as another spelling of a name;
/// - on the profile folder or above it ([`FolderRefusal::HoldsTheProfile`]);
/// - on, over or in `<data_dir>\sandbox` ([`FolderRefusal::OverlapsSandboxRecords`]);
/// - a deny, or a path entirely outside `<data_dir>`, is fine;
/// - a path that is `<data_dir>` or holds it is refused, and inside it
///   ([`FolderRefusal::OverlapsDataFolder`] for anything not listed here):
///   - writable: the state folder exactly one level below `<data_dir>\agent-state\<matter>`
///     (not `notes`), the notes folder exactly `<data_dir>\agent-state\<matter>\notes`, the
///     credential link inside that state folder, and the Claude store exactly
///     `<data_dir>\agents\claude-acp\home` and its allowed names, `matter` being this
///     launch's matter id, never a string of the policy's;
///   - read and run: inside one agent's folder under `<data_dir>\agents`, never in its `home`
///     unless it is that Claude store.
fn check_target(admitted: &Admitted<'_>, grant: &Grant, matter: MatterId) -> Option<FolderRefusal> {
    let policy = admitted.policy();
    let path = &grant.path;
    if odd_spelling(path) {
        return Some(FolderRefusal::OddSpelling);
    }
    if policy.home().is_some_and(|home| in_folder(home, path)) {
        return Some(FolderRefusal::HoldsTheProfile);
    }
    let data = admitted.data_dir();
    let sandbox = data.join(SANDBOX_DIR);
    if in_folder(path, &sandbox) || in_folder(&sandbox, path) {
        return Some(FolderRefusal::OverlapsSandboxRecords);
    }
    if grant.rights.is_deny() || !(in_folder(path, data) || in_folder(data, path)) {
        return None;
    }
    let allowed = in_folder(path, data) && !same_path(path, data) && {
        let mine = data.join(AGENT_STATE_DIR).join(matter.to_string());
        let store = data
            .join(AGENTS_DIR)
            .join(CLAUDE_AGENT)
            .join(AGENT_HOME_DIR);
        let state_ok = |state: &Path| {
            state
                .parent()
                .is_some_and(|parent| same_path(parent, &mine))
                && !state
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case(NOTES_DIR))
        };
        match grant.rights {
            Rights::ReadAndExecute => same_path(path, &store) || in_agent_install(path, data),
            Rights::Modify | Rights::CreateFiles => match grant.kind {
                PathKind::State => state_ok(path),
                PathKind::Notes => same_path(path, &mine.join(NOTES_DIR)),
                PathKind::CredentialLink => {
                    state_ok(policy.state()) && in_folder(path, policy.state())
                }
                PathKind::ClaudeStore => same_path(path, &store),
                PathKind::ClaudeStoreFile => path.parent().is_some_and(|p| same_path(p, &store)),
                PathKind::Matter
                | PathKind::Runtime
                | PathKind::Read
                | PathKind::AllowedFolder
                | PathKind::ReadOnlyInMatter
                | PathKind::ReadOnlyInState
                | PathKind::OtherMatter => false,
            },
            Rights::DenyWrite | Rights::DenyReadWrite => true,
        }
    };
    (!allowed).then_some(FolderRefusal::OverlapsDataFolder)
}

/// Whether `path` is inside one agent's folder under `<data_dir>\agents` (not the folder
/// itself, which holds the home) and outside its `home`.
fn in_agent_install(path: &Path, data: &Path) -> bool {
    let agents = data.join(AGENTS_DIR);
    if !in_folder(path, &agents) {
        return false;
    }
    let below: Vec<Component<'_>> = path
        .components()
        .skip(agents.components().count())
        .collect();
    let in_home = below
        .get(1)
        .is_some_and(|part| part.as_os_str().eq_ignore_ascii_case(AGENT_HOME_DIR));
    below.len() >= 2 && !in_home
}

/// Whether `a` and `b` are the same path, by [`in_folder`]'s comparison.
fn same_path(a: &Path, b: &Path) -> bool {
    a.components().count() == b.components().count() && in_folder(a, b)
}

/// Whether a name in `path` is a spelling Windows reads as another name: one ending in `.` or
/// a space, or one holding `:` (a stream; the drive's colon is in the prefix, not a name).
/// Short 8.3 names need no rule here: every policy path, and the data folder, is canonical
/// through the final-path API (`dunce::canonicalize`, `GetFinalPathNameByHandleW` on Windows),
/// which gives long names, and a `~` is an ordinary character (`Draft~2`).
pub(super) fn odd_spelling(path: &Path) -> bool {
    path.components().any(|part| {
        let Component::Normal(name) = part else {
            return false;
        };
        let name = name.to_string_lossy();
        name.ends_with('.') || name.ends_with(' ') || name.contains(':')
    })
}

/// `<data_dir>\agents`: each agent's install folder, its runtime and its home.
pub(super) const AGENTS_DIR: &str = "agents";

/// `<data_dir>\agent-state`: each matter's state and notes folders, by matter id.
const AGENT_STATE_DIR: &str = "agent-state";

/// A matter's notes folder in `<data_dir>\agent-state\<matter>`.
const NOTES_DIR: &str = "notes";

/// The one agent whose home may be the Claude store: Clauseo's own Claude sign-in.
const CLAUDE_AGENT: &str = "claude-acp";

/// An agent's home in its install folder, `<data_dir>\agents\<agent>\home`: its sign-in.
const AGENT_HOME_DIR: &str = "home";

/// The entries planned so far.
struct Plan<'s> {
    sids: &'s Sids,
    grants: Vec<Grant>,
}

impl Plan<'_> {
    /// An allow for the user's SID and the same for the matter's.
    fn pair(&mut self, path: &Path, rights: Rights, inherit: Inherit, kind: PathKind) {
        for sid in [&self.sids.user, &self.sids.matter] {
            self.push(path, sid.clone(), rights, inherit, kind);
        }
    }

    /// An entry for the matter's SID alone (a deny).
    fn matter_only(&mut self, path: &Path, rights: Rights, inherit: Inherit, kind: PathKind) {
        self.push(path, self.sids.matter.clone(), rights, inherit, kind);
    }

    fn push(&mut self, path: &Path, sid: Sid, rights: Rights, inherit: Inherit, kind: PathKind) {
        let grant = Grant {
            path: path.to_owned(),
            sid,
            rights,
            inherit,
            kind,
        };
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

#[cfg(test)]
mod tests;
