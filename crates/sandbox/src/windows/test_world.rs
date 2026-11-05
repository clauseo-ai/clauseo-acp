//! The tier B tests' world: a matter, its state and notes folders, a data folder, a home with a
//! `.claude` folder, other matters, all in one temp folder; the two SIDs, matter ids, and a fake
//! DACL reader.

use std::cell::RefCell;
use std::io;
use std::path::{Component, Path, PathBuf};

use clauseo_protocol::MatterId;

use super::acl_plan::Subjects;
use super::admit::FolderList;
use super::sids::{Reserved, Sid};
use crate::test_support::TempDir;
use crate::{Policy, PolicyBuilder};

/// This matter's pool user's SID in the tests.
pub(super) const USER_SID: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
/// Two other matters' pool users.
pub(super) const OTHER_POOL_SIDS: [&str; 2] = [
    "S-1-5-21-1111111111-2222222222-3333333333-1002",
    "S-1-5-21-1111111111-2222222222-3333333333-1003",
];
/// The `ClauseoSandboxUsers` group.
pub(super) const GROUP_SID: &str = "S-1-5-21-1111111111-2222222222-3333333333-1010";
/// This matter's SID in the tests.
pub(super) const MATTER_SID: &str = "S-1-5-21-2626480971-3383066999-180375651-600177987";

/// This matter's state folder, under its id as the plan expects, relative to the temp folder.
pub(super) const STATE: &str = "data/agent-state/01890a5d-ac96-774b-bcce-b302099a8057/claude";
/// This matter's notes folder.
pub(super) const NOTES: &str = "data/agent-state/01890a5d-ac96-774b-bcce-b302099a8057/notes";
/// Clauseo's Claude sign-in home, the one Claude store the plan allows in the data folder.
pub(super) const STORE: &str = "data/agents/claude-acp/home";

/// This matter's id.
pub(super) fn matter_one() -> MatterId {
    "01890a5d-ac96-774b-bcce-b302099a8057".parse().unwrap()
}

/// Another matter's id.
pub(super) fn matter_two() -> MatterId {
    "01890a5d-ac96-774b-8cce-b302099a8058".parse().unwrap()
}

/// The pool user's SID.
pub(super) fn user_sid() -> Sid {
    Sid::parse(USER_SID).unwrap()
}

/// The SIDs a ledger trusts: every pool user and the group.
pub(super) fn trusted() -> Vec<Sid> {
    let mut sids = vec![user_sid(), Sid::parse(GROUP_SID).unwrap()];
    sids.extend(OTHER_POOL_SIDS.iter().map(|s| Sid::parse(s).unwrap()));
    sids
}

/// The reserved accounts: a stand-in for the lawyer's own account (another domain than the
/// matter SID's) and the sandbox user.
pub(super) fn reserved() -> Reserved {
    Reserved::new([Sid::parse("S-1-5-21-7-8-9-1001").unwrap(), user_sid()])
}

/// The subjects: this pool user, two others, the group; no matter SID.
pub(super) fn sids() -> Subjects {
    Subjects {
        pool_user: user_sid(),
        matter: None,
        other_pool_users: OTHER_POOL_SIDS
            .iter()
            .map(|s| Sid::parse(s).unwrap())
            .collect(),
        pool_group: Sid::parse(GROUP_SID).unwrap(),
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
            STATE,
            NOTES,
            STORE,
            "data/agent-state/01890a5d-ac96-774b-8cce-b302099a8058/notes",
            "data/updates",
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
        Policy::builder(self.at("matter"), self.at(STATE))
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
        self.dir.file(&format!("{STATE}/auth.json"), "{}");
        self.dir.file(&format!("{STATE}/settings.json"), "{}");
        self.builder()
            .notes(self.at(NOTES))
            .credential_link(self.at(&format!("{STATE}/auth.json")))
            .read_only_in_state(self.at(&format!("{STATE}/settings.json")))
            .claude_store(self.at(STORE))
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

/// A folder lister that gives `listing` for any parent, or fails (`PermissionDenied`) when
/// `failing`; records each parent it is asked about.
#[derive(Default)]
pub(super) struct FakeFolders {
    pub(super) listing: Vec<PathBuf>,
    pub(super) failing: bool,
    pub(super) asked: RefCell<Vec<PathBuf>>,
}

impl FolderList for FakeFolders {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        self.asked.borrow_mut().push(parent.to_owned());
        if self.failing {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        } else {
            Ok(self.listing.clone())
        }
    }
}
