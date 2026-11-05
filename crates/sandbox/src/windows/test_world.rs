//! The tier B tests' world: a matter, its state and notes folders, a data folder, a home with a
//! `.claude` folder, other matters, all in one temp folder; the two SIDs, matter ids, and a fake
//! DACL reader.

use std::cell::RefCell;
use std::io;
use std::path::{Component, Path, PathBuf};

use clauseo_protocol::MatterId;

use super::acl_plan::Sids;
use super::admit::GroupAccess;
use super::sids::{Reserved, Sid};
use crate::test_support::TempDir;
use crate::{Policy, PolicyBuilder};

/// The `ClauseoSandbox` user's SID in the tests.
pub(super) const USER_SID: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
/// This matter's SID in the tests.
pub(super) const MATTER_SID: &str = "S-1-5-21-2626480971-3383066999-180375651-600177987";

/// This matter's id.
pub(super) fn matter_one() -> MatterId {
    "01890a5d-ac96-774b-bcce-b302099a8057".parse().unwrap()
}

/// Another matter's id.
pub(super) fn matter_two() -> MatterId {
    "01890a5d-ac96-774b-8cce-b302099a8058".parse().unwrap()
}

/// The sandbox user's SID.
pub(super) fn user_sid() -> Sid {
    Sid::parse(USER_SID).unwrap()
}

/// The reserved accounts: a stand-in for the lawyer's own account (another domain than the
/// matter SID's) and the sandbox user.
pub(super) fn reserved() -> Reserved {
    Reserved::new([Sid::parse("S-1-5-21-7-8-9-1001").unwrap(), user_sid()])
}

/// Both SIDs.
pub(super) fn sids() -> Sids {
    Sids {
        user: Sid::parse(USER_SID).unwrap(),
        matter: Sid::parse(MATTER_SID).unwrap(),
    }
}

pub(super) struct World {
    pub(super) dir: TempDir,
}

impl World {
    pub(super) fn new(test: &str) -> World {
        let dir = TempDir::new(test);
        for name in [
            "matter",
            "data/agent-state/m1/claude",
            "data/agent-state/m1/notes",
            "data/claude-home",
            "runtime/node",
            "home/.claude",
            "elsewhere/Precedents",
            "other/m2",
            "other/m3",
        ] {
            dir.dir(name);
        }
        World { dir }
    }

    pub(super) fn at(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// `rel` canonical, as the policy holds it.
    pub(super) fn canonical(&self, rel: &str) -> PathBuf {
        dunce::canonicalize(self.at(rel)).unwrap()
    }

    /// The matter and its state folder, with the home and the data folder hidden.
    pub(super) fn builder(&self) -> PolicyBuilder {
        Policy::builder(self.at("matter"), self.at("data/agent-state/m1/claude"))
            .home(self.at("home"))
            .hidden(self.at("data"))
    }

    /// Every kind of path the plan grants on: a repository and an over-cap file in the matter,
    /// notes, a credential link, a read-only path in the state folder, Clauseo's Claude home,
    /// runtime, read and allowed paths.
    pub(super) fn full_policy(&self) -> Policy {
        self.dir.file("matter/.git/HEAD", "ref: refs/heads/main\n");
        self.dir
            .file("matter/.codex/config.toml", "model = \"x\"\n");
        self.dir.file("matter/CLAUDE.md", "# Instructions\n");
        let big = self.dir.file("matter/big.pdf", "large");
        self.dir.file("data/agent-state/m1/claude/auth.json", "{}");
        self.dir
            .file("data/agent-state/m1/claude/settings.json", "{}");
        self.builder()
            .notes(self.at("data/agent-state/m1/notes"))
            .credential_link(self.at("data/agent-state/m1/claude/auth.json"))
            .read_only_in_state(self.at("data/agent-state/m1/claude/settings.json"))
            .claude_store(self.at("data/claude-home"))
            .runtime(self.at("runtime/node"))
            .read(self.at("elsewhere"))
            .allowed_folder(self.at("elsewhere/Precedents"))
            .over_cap([big])
            .build()
            .unwrap()
    }

    /// `path` relative to the temp folder, written with `/`, so snapshots match on every OS.
    pub(super) fn relative(&self, path: &Path) -> String {
        let root = dunce::canonicalize(self.dir.path()).unwrap();
        let Ok(rest) = path.strip_prefix(&root) else {
            return format!("<outside the temp folder: {}>", path.display());
        };
        let parts: Vec<String> = rest
            .components()
            .filter_map(|c| match c {
                Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                Component::Prefix(_)
                | Component::RootDir
                | Component::CurDir
                | Component::ParentDir => None,
            })
            .collect();
        format!("<root>/{}", parts.join("/"))
    }
}

/// A DACL reader that says a group can reach the folders in `open`, fails on those in
/// `failing` (`PermissionDenied`) and on those in `missing` (`NotFound`), and says no to the
/// rest. Records each folder it is asked about.
#[derive(Default)]
pub(super) struct FakeGroups {
    pub(super) open: Vec<PathBuf>,
    pub(super) failing: Vec<PathBuf>,
    pub(super) missing: Vec<PathBuf>,
    pub(super) asked: RefCell<Vec<PathBuf>>,
}

impl GroupAccess for FakeGroups {
    fn group_can_reach(&self, folder: &Path) -> io::Result<bool> {
        self.asked.borrow_mut().push(folder.to_owned());
        if self.failing.iter().any(|f| f == folder) {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        } else if self.missing.iter().any(|f| f == folder) {
            Err(io::Error::from(io::ErrorKind::NotFound))
        } else {
            Ok(self.open.iter().any(|f| f == folder))
        }
    }
}
