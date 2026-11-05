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

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::admit::{Admitted, PathKind};
use crate::policy::under;
use crate::{Error, PathRule};

/// A SID in its string form (`S-1-5-21-…`). Only the shape is checked: `S-1-`, then at least
/// one more part, every part decimal. S15's PR2 brings the SID types this stands in for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sid(String);

impl Sid {
    /// `text` as a SID, or `None` when it isn't one's string form.
    pub fn new(text: impl Into<String>) -> Option<Sid> {
        let text = text.into();
        let mut parts = text.split('-');
        let shaped = parts.next() == Some("S") && parts.next() == Some("1") && {
            let rest: Vec<&str> = parts.collect();
            !rest.is_empty()
                && rest.iter().all(|part| {
                    !part.is_empty() && part.len() <= 10 && part.bytes().all(|b| b.is_ascii_digit())
                })
        };
        shaped.then_some(Sid(text))
    }

    /// The string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Sid {
    type Error = String;

    fn try_from(text: String) -> Result<Sid, String> {
        Sid::new(text).ok_or_else(|| "not a SID's string form".to_owned())
    }
}

impl From<Sid> for String {
    fn from(sid: Sid) -> String {
        sid.0
    }
}

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

/// One permission entry to write.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Grant {
    /// Where, canonical as the policy's paths are.
    pub path: PathBuf,
    /// Whom.
    pub sid: Sid,
    /// What.
    pub rights: Rights,
    /// How far down.
    pub inherit: Inherit,
    /// Why: which of the policy's paths this is.
    pub kind: PathKind,
}

/// The plan for `admitted`'s policy with `sids`, in a fixed order: the writable folders, the
/// credential link, the Claude store, the read paths, the read-only denies, then D8's denies.
/// Each entry appears once.
///
/// Refuses ([`Error::Policy`], naming the kind of path but not the path) when the two SIDs are
/// the same, which would collapse every pair, or when any entry would land in `<home>\.claude`.
pub fn plan(admitted: &Admitted<'_>, sids: &Sids) -> Result<Vec<Grant>, Error> {
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
    Ok(plan.grants)
}

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
fn in_folder(path: &Path, folder: &Path) -> bool {
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
