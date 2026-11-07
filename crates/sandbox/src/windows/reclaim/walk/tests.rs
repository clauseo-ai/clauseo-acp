//! `--reclaim`'s walk over a fake tree: only what a pool user owns and the desktop user can't
//! hand back changes owner; what the lawyer owns is reported, never touched; links and files
//! with more than one link are skipped; no call that grants exists, and no DACL changes. What it
//! may reveal and enter is bounded by access: a root outside every recorded matter is refused,
//! and a folder neither a pool user owns nor the owner can list is never entered or counted.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use super::*;

const OWNER: &str = "S-1-5-21-1-2-3-1001";
const POOL_A: &str = "S-1-5-21-1-2-3-1101";
const POOL_B: &str = "S-1-5-21-1-2-3-1102";
const COLLEAGUE: &str = "S-1-5-21-1-2-3-1201";
/// What the owner gets on its own folder.
const ALL: u32 = HAND_BACK | LIST;

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// `name` as an absolute path on every OS.
fn p(name: &str) -> PathBuf {
    std::env::temp_dir().join("clauseo-reclaim-walk").join(name)
}

/// One object of the fake tree.
#[derive(Debug, Clone)]
struct Item {
    path: PathBuf,
    kind: Kind,
    owner: Sid,
    /// What the desktop user's token gets on it.
    granted: u32,
    /// Its DACL, opaque: only the test writes it.
    dacl: &'static str,
    /// Its open fails, as an access refusal would.
    unopenable: bool,
    /// Its open meets a reparse point (`OBJ_DONT_REPARSE`).
    reparse: bool,
}

/// A tree of items by path; every call is recorded.
#[derive(Debug, Default)]
struct Fake {
    items: Vec<Item>,
    owner_writes: Vec<(PathBuf, Sid)>,
    fail_owner_write: Option<PathBuf>,
    /// Paths that resolve to another final path (a short name, a junction on the way).
    aliases: BTreeMap<PathBuf, PathBuf>,
    /// Every item whose owner, rights or children were asked for.
    asked: Vec<PathBuf>,
}

impl Fake {
    fn add(&mut self, name: &str, kind: Kind, owner: &str, granted: u32) -> &mut Item {
        self.items.push(Item {
            path: p(name),
            kind,
            owner: sid(owner),
            granted,
            dacl: "D:(A;;FA;;;OW)",
            unopenable: false,
            reparse: false,
        });
        self.items.last_mut().unwrap()
    }

    fn owner_of(&self, name: &str) -> Sid {
        let found = self.items.iter().find(|i| i.path == p(name));
        found.unwrap().owner.clone()
    }

    fn asked_below(&self, name: &str) -> bool {
        self.asked
            .iter()
            .any(|a| a.starts_with(p(name)) && *a != p(name))
    }
}

impl OwnerTree for Fake {
    type Node = usize;
    type Name = usize;

    fn resolve(&mut self, path: &Path) -> io::Result<PathBuf> {
        Ok(self
            .aliases
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_owned()))
    }

    fn open_root(&mut self, root: &Path) -> io::Result<usize> {
        let at = self.items.iter().position(|i| i.path == root);
        at.ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn names(&mut self, folder: &usize) -> io::Result<Vec<usize>> {
        let parent = self.items[*folder].path.clone();
        self.asked.push(parent.join("*"));
        let in_it = self.items.iter().enumerate();
        Ok(in_it
            .filter(|(_, i)| i.path.parent() == Some(parent.as_path()))
            .map(|(at, _)| at)
            .collect())
    }

    fn open(&mut self, _folder: &usize, name: &usize) -> io::Result<Option<usize>> {
        match &self.items[*name] {
            item if item.unopenable => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            item if item.reparse => Ok(None),
            _ => Ok(Some(*name)),
        }
    }

    fn kind(&self, node: &usize) -> Kind {
        self.items[*node].kind
    }

    fn owner(&mut self, node: &usize) -> io::Result<Sid> {
        self.asked.push(self.items[*node].path.clone());
        Ok(self.items[*node].owner.clone())
    }

    fn granted(&mut self, node: &usize) -> io::Result<u32> {
        Ok(self.items[*node].granted)
    }

    fn set_owner(&mut self, node: &usize, owner: &Sid) -> io::Result<()> {
        let path = self.items[*node].path.clone();
        if self.fail_owner_write.as_ref() == Some(&path) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        self.owner_writes.push((path, owner.clone()));
        self.items[*node].owner = owner.clone();
        Ok(())
    }
}

/// A matter with every case the walk meets.
fn matter() -> Fake {
    let mut fake = Fake::default();
    fake.add("m", Kind::Folder, OWNER, ALL);
    // Another pool user's file the lawyer may read and re-permission, not re-own.
    fake.add("m/moved.txt", Kind::File, POOL_B, READ_CONTROL | WRITE_DAC);
    // A pool user's folder that shuts the lawyer out, with a file in it.
    fake.add("m/shut", Kind::Folder, POOL_A, 0);
    fake.add("m/shut/inner.txt", Kind::File, POOL_A, 0);
    // A pool user's file the lawyer can hand back: left to the hand-back.
    fake.add("m/open.txt", Kind::File, POOL_A, HAND_BACK);
    // The lawyer's own file that shuts the lawyer out, and a colleague's.
    fake.add("m/mine.txt", Kind::File, OWNER, 0);
    fake.add("m/colleague.txt", Kind::File, COLLEAGUE, READ_CONTROL);
    // A junction a pool user made, and a pool user's file with two links.
    fake.add("m/junction", Kind::Link, POOL_A, 0);
    fake.add("m/junction/beyond.txt", Kind::File, POOL_A, 0);
    fake.add("m/linked.txt", Kind::Linked, POOL_A, 0);
    // A name whose open met a reparse point: a junction swapped in after the listing.
    fake.add("m/swapped", Kind::Folder, POOL_A, 0).reparse = true;
    fake
}

fn pool() -> Vec<Sid> {
    vec![sid(POOL_A), sid(POOL_B)]
}

/// The walk of `root` for the owner, with `m` the one recorded matter root.
fn walk(fake: &mut Fake, root: &str, whole: bool) -> Reclaimed {
    walk_as(fake, root, whole, &sid(OWNER))
}

fn walk_as(fake: &mut Fake, root: &str, whole: bool, owner: &Sid) -> Reclaimed {
    let (pool, bounds) = (pool(), [p("m")]);
    let scope = Scope {
        pool: &pool,
        owner,
        bounds: &bounds,
        profiles: &[p("Users/ClauseoSb01")],
    };
    take_ownership(fake, &p(root), whole, &scope)
}

#[test]
fn only_what_a_pool_user_owns_and_the_lawyer_cant_hand_back_changes_owner() {
    let mut fake = matter();
    let before: Vec<&'static str> = fake.items.iter().map(|i| i.dacl).collect();

    let seen = walk(&mut fake, "m", true);

    let mut taken: Vec<PathBuf> = fake.owner_writes.iter().map(|(p, _)| p.clone()).collect();
    taken.sort();
    assert_eq!(taken, ["m/moved.txt", "m/shut", "m/shut/inner.txt"].map(p));
    assert!(fake.owner_writes.iter().all(|(_, o)| *o == sid(OWNER)));
    // The hand-back's own job, the lawyer's file, the colleague's, the link and what is behind
    // it, and the linked file all keep their owner.
    assert_eq!(fake.owner_of("m/open.txt"), sid(POOL_A));
    assert_eq!(fake.owner_of("m/mine.txt"), sid(OWNER));
    assert_eq!(fake.owner_of("m/colleague.txt"), sid(COLLEAGUE));
    assert_eq!(fake.owner_of("m/junction/beyond.txt"), sid(POOL_A));
    assert_eq!(fake.owner_of("m/linked.txt"), sid(POOL_A));
    // No DACL changed: the seam has no call that could.
    let after: Vec<&'static str> = fake.items.iter().map(|i| i.dacl).collect();
    assert_eq!(after, before);
    let want = Reclaimed {
        visited: 7,
        taken: 3,
        skipped: 3,
        // The lawyer's shut file is reported; the colleague's, which the lawyer may still
        // read, isn't counted.
        left: 1,
        failed: 0,
        roots_refused: 0,
    };
    assert_eq!(seen, want);
}

/// A folder no pool user owns that the owner can't list is never entered and never counted,
/// whatever is in it: Backup never opens what neither could.
#[test]
fn a_folder_neither_a_pool_user_owns_nor_the_owner_can_list_is_not_entered_or_counted() {
    let mut fake = Fake::default();
    fake.add("m", Kind::Folder, OWNER, ALL);
    fake.add("m/mine", Kind::Folder, OWNER, READ_CONTROL | WRITE_DAC);
    fake.add("m/mine/agent.txt", Kind::File, POOL_A, 0);
    fake.add("m/colleague", Kind::Folder, COLLEAGUE, 0);
    fake.add("m/colleague/secret.txt", Kind::File, POOL_A, 0);

    let seen = walk(&mut fake, "m", true);

    assert_eq!(fake.owner_writes, []);
    assert!(!fake.asked_below("m/mine") && !fake.asked_below("m/colleague"));
    let want = Reclaimed {
        visited: 1,
        ..Reclaimed::default()
    };
    assert_eq!(seen, want, "only the root counts");
}

/// A folder the owner can list is entered whoever owns it: what a pool user made there is
/// taken.
#[test]
fn a_folder_the_owner_can_list_is_entered() {
    let mut fake = Fake::default();
    fake.add("m", Kind::Folder, OWNER, ALL);
    fake.add("m/colleague", Kind::Folder, COLLEAGUE, READ_CONTROL | LIST);
    fake.add("m/colleague/agent.txt", Kind::File, POOL_A, 0);

    let seen = walk(&mut fake, "m", true);

    assert_eq!(fake.owner_writes.len(), 1);
    assert_eq!(fake.owner_of("m/colleague"), sid(COLLEAGUE));
    assert_eq!((seen.visited, seen.taken), (3, 1));
}

/// A root outside every recorded matter root (a system folder, another user's, an unrecorded
/// matter), or one a junction on the way or a short name resolves outside, is refused with
/// nothing read below it.
#[test]
fn a_root_outside_every_recorded_matter_is_refused() {
    for outside in [
        "Windows",
        "Users/colleague/Documents",
        "unrecorded-matter",
        "m-two",
    ] {
        let mut fake = matter();
        fake.add(outside, Kind::Folder, POOL_A, 0);
        fake.add(&format!("{outside}/inner.txt"), Kind::File, POOL_A, 0);
        let seen = walk(&mut fake, outside, true);
        let refused = Reclaimed {
            roots_refused: 1,
            ..Reclaimed::default()
        };
        assert_eq!(seen, refused, "{outside}");
        assert_eq!(fake.asked, Vec::<PathBuf>::new(), "{outside}");
    }
    let mut fake = matter();
    fake.add("Windows", Kind::Folder, POOL_A, 0);
    fake.aliases.insert(p("m/link-to-windows"), p("Windows"));
    let seen = walk(&mut fake, "m/link-to-windows", true);
    assert_eq!(seen.roots_refused, 1);
    assert_eq!(fake.owner_writes, []);
}

/// A short name resolves, by handle, to the final path the bound is checked on.
#[test]
fn a_short_name_inside_a_matter_resolves_and_is_walked() {
    let mut fake = matter();
    fake.aliases.insert(p("M~1/SHUT~1"), p("m/shut"));
    let seen = walk(&mut fake, "M~1/SHUT~1", true);
    assert_eq!((seen.taken, seen.roots_refused), (2, 0));
}

#[test]
fn a_root_no_pool_user_owns_and_the_owner_cant_list_is_refused_unread() {
    let mut fake = Fake::default();
    fake.add("m", Kind::Folder, OWNER, ALL);
    fake.add("m/closed", Kind::Folder, COLLEAGUE, READ_CONTROL);
    fake.add("m/closed/agent.txt", Kind::File, POOL_A, 0);

    let seen = walk(&mut fake, "m/closed", true);

    assert_eq!(seen.roots_refused, 1);
    assert_eq!((seen.visited, seen.taken), (0, 0));
    assert!(!fake.asked_below("m/closed"));
}

#[test]
fn a_root_in_a_pool_users_profile_is_refused() {
    let mut fake = matter();
    let bounds = [p("Users")];
    fake.add("Users/ClauseoSb01/NTUSER.DAT", Kind::File, POOL_A, 0);
    let pool = pool();
    let owner = sid(OWNER);
    let scope = Scope {
        pool: &pool,
        owner: &owner,
        bounds: &bounds,
        profiles: &[p("Users/ClauseoSb01")],
    };
    let seen = take_ownership(&mut fake, &p("Users/ClauseoSb01/NTUSER.DAT"), false, &scope);
    assert_eq!(seen.roots_refused, 1);
    let seen = take_ownership(&mut fake, &p("Users"), true, &scope);
    assert_eq!(seen.roots_refused, 1, "it holds a profile");
}

#[test]
fn a_root_alone_is_taken_without_its_tree() {
    let mut fake = Fake::default();
    fake.add("m", Kind::Folder, POOL_B, READ_CONTROL);
    fake.add("m/agent.txt", Kind::File, POOL_A, 0);

    let seen = walk(&mut fake, "m", false);

    assert_eq!((seen.visited, seen.taken), (1, 1));
    assert_eq!(fake.owner_of("m"), sid(OWNER));
    assert_eq!(fake.owner_of("m/agent.txt"), sid(POOL_A));
}

#[test]
fn a_root_that_is_a_link_or_a_linked_file_is_skipped() {
    for kind in [Kind::Link, Kind::Linked] {
        let mut fake = Fake::default();
        fake.add("m", kind, POOL_A, 0);
        let seen = walk(&mut fake, "m", true);
        assert_eq!((seen.skipped, seen.taken), (1, 0), "{kind:?}");
        assert_eq!(fake.owner_writes, [], "{kind:?}");
    }
}

#[test]
fn failures_are_counted_and_the_walk_goes_on() {
    let mut fake = matter();
    fake.add("m/gone", Kind::File, POOL_A, 0).unopenable = true;
    fake.fail_owner_write = Some(p("m/moved.txt"));

    let seen = walk(&mut fake, "m", true);

    assert_eq!((seen.taken, seen.failed), (2, 2), "{seen:?}");
    assert_eq!(fake.owner_of("m/moved.txt"), sid(POOL_B));
}

#[test]
fn a_missing_root_is_nothing_and_an_owner_in_the_pool_takes_nothing() {
    let mut fake = matter();
    let none = walk(&mut fake, "m/elsewhere", true);
    assert_eq!(none, Reclaimed::default());

    let pooled = walk_as(&mut fake, "m", true, &sid(POOL_A));
    assert_eq!(pooled.roots_refused, 1);
    assert_eq!(fake.owner_writes, []);
}
