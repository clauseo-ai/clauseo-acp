//! The fence paths of other matters: a sibling whole, a child of the session's matter whole, a
//! parent's siblings along the way down (never the session's own matter or `<data>`), an
//! invited matter's place inside a fenced one left open, and the innermost-matter match.

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::{MatterFence, Other, innermost};
use crate::pipeline::tests::support::TempDir;
use crate::write;

fn id(n: u128) -> MatterId {
    let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
    MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
}

/// `root` with folders `names` made (`a/b` makes both).
fn tree(test: &str, names: &[&str]) -> (TempDir, PathBuf) {
    let dir = TempDir::new(test);
    let root = dunce::canonicalize(dir.path()).unwrap();
    for name in names {
        write::create_dir_all(&root.join(name)).unwrap();
    }
    (dir, root)
}

/// The fence for a session whose matter is `root/own` and data folder `root/data`, of
/// `others` (folder below `root`, invited).
fn fence(root: &Path, own: &str, others: &[(&str, bool)]) -> MatterFence {
    let asked = others
        .iter()
        .zip(1..)
        .map(|((folder, invited), n)| {
            Other::new(id(n), format!("M{n}"), root.join(folder), *invited)
        })
        .collect();
    let (own, data) = (root.join(own), root.join("data"));
    MatterFence::asked(asked).resolve(&[own.as_path()], &[data.as_path()])
}

fn paths(fence: &MatterFence, n: usize) -> Vec<PathBuf> {
    let mut paths = fence.others[n].paths.clone();
    paths.sort();
    paths
}

#[test]
fn a_sibling_and_a_child_are_fenced_whole() {
    let (_dir, root) = tree("mfence-sibling", &["own/child", "other", "data"]);
    let fence = fence(&root, "own", &[("other", false), ("own/child", false)]);
    assert_eq!(paths(&fence, 0), [root.join("other")]);
    assert_eq!(paths(&fence, 1), [root.join("own/child")]);
    assert_eq!(fence.denied_reads().count(), 2);
}

#[test]
fn a_parent_is_fenced_by_its_siblings_along_the_way_never_the_own_matter() {
    let (_dir, root) = tree(
        "mfence-parent",
        &[
            "clients/acme/deal",
            "clients/acme/old",
            "clients/zed",
            "data",
        ],
    );
    write::write_file(&root.join("clients/notes.txt"), b"x").unwrap();
    let fence = fence(&root, "clients/acme/deal", &[("clients", false)]);
    assert_eq!(
        paths(&fence, 0),
        [
            root.join("clients/acme/old"),
            root.join("clients/notes.txt"),
            root.join("clients/zed"),
        ]
    );
    // `<data>` inside a parent is never denied either.
    let fence = self::fence(&root, "clients/acme/deal", &[("", false)]);
    let denied = paths(&fence, 0);
    assert!(
        !denied
            .iter()
            .any(|path| root.join("data").starts_with(path)),
        "{denied:?}"
    );
    assert!(
        !denied
            .iter()
            .any(|path| root.join("clients/acme/deal").starts_with(path))
    );
    assert!(denied.contains(&root.join("clients/zed")), "{denied:?}");
}

#[test]
fn the_own_matter_is_never_fenced() {
    let (_dir, root) = tree("mfence-own", &["own", "data"]);
    let fence = fence(&root, "own", &[("own", false)]);
    assert!(fence.denied_edits().next().is_none());
    // A folder that is not absolute is in no rule.
    let relative = MatterFence::asked(vec![Other::new(id(9), "R".into(), "rel".into(), false)])
        .resolve(&[root.join("own").as_path()], &[]);
    assert!(relative.denied_edits().next().is_none());
}

#[test]
fn an_invite_opens_reads_only_and_its_place_in_a_fenced_parent() {
    let (_dir, root) = tree("mfence-invite", &["own", "shelf/b/x", "shelf/c", "data"]);
    let fence = fence(&root, "own", &[("shelf", false), ("shelf/b", true)]);
    // The fenced parent leaves the invited matter out; the invite keeps its edit deny.
    assert_eq!(paths(&fence, 0), [root.join("shelf/c")]);
    assert_eq!(paths(&fence, 1), [root.join("shelf/b")]);
    let reads: Vec<&Path> = fence.denied_reads().collect();
    assert_eq!(reads, [root.join("shelf/c")]);
    let edits: Vec<&Path> = fence.denied_edits().collect();
    assert_eq!(edits, [root.join("shelf/c"), root.join("shelf/b")]);
    let allowed: Vec<&Path> = fence.read_allowed().collect();
    assert_eq!(allowed, [root.join("shelf/b")]);
    let folders: Vec<&Path> = fence.fenced_folders().collect();
    assert_eq!(folders, [root.join("shelf")]);
}

#[test]
fn a_refused_path_names_the_innermost_other_matter_but_never_the_own() {
    let (_dir, root) = tree("mfence-innermost", &["shelf/own", "shelf/b/c", "data"]);
    let fence = fence(
        &root,
        "shelf/own",
        &[("shelf", false), ("shelf/b", false), ("shelf/b/c", false)],
    );
    let named = |path: &str| innermost(&fence, &root.join(path)).map(|other| other.name.clone());
    assert_eq!(named("shelf/b/c/memo.docx").as_deref(), Some("M3"));
    assert_eq!(named("shelf/b/memo.docx").as_deref(), Some("M2"));
    assert_eq!(named("shelf/top.docx").as_deref(), Some("M1"));
    assert_eq!(named("shelf/own/draft.docx"), None);
    assert_eq!(named("elsewhere/x"), None);
    // A name that only starts with a matter's is no part of it.
    assert_eq!(named("shelf/bob/memo.docx").as_deref(), Some("M1"));
}

/// A path that only ends in a matter's spelling (`/mnt/<matter>/…`) is no part of it: names
/// match whole from the root.
#[cfg(unix)]
#[test]
fn a_path_ending_in_a_matters_spelling_is_not_in_it() {
    let (_dir, root) = tree("mfence-suffix", &["own", "b", "data"]);
    let fence = fence(&root, "own", &[("b", false)]);
    let b = root.join("b/memo.docx");
    assert!(innermost(&fence, &b).is_some());
    let mounted = PathBuf::from(format!("/mnt{}", b.display()));
    assert!(innermost(&fence, &mounted).is_none());
}

/// Names compare folded, the drive letter too, so another matter stored as `c:\clients` is the
/// parent of this one at `C:\Clients\Acme`, never a sibling to fence whole.
#[test]
fn a_drive_letter_in_another_case_still_holds_the_matter() {
    let parent = super::key_as(Path::new("c:/clients"), true);
    let own = super::key_as(Path::new("C:/Clients/Acme"), true);
    assert!(own.starts_with(&parent), "{own:?} {parent:?}");
    #[cfg(windows)]
    {
        let verbatim = super::key_as(Path::new(r"\\?\C:\Clients\Acme"), true);
        assert_eq!(verbatim, own);
    }
}

/// A matter inside the session's own, then one deeper down: the own matter is shut to the
/// sandbox (`own_shut`), and every matter inside, invited or not, is listed for a write deny.
#[test]
fn a_fenced_matter_inside_the_own_one_shuts_it_to_the_sandbox() {
    let (_dir, root) = tree(
        "mfence-own-shut",
        &[
            "own/Sub/x",
            "own/Drafts",
            "own/p/q/Deep",
            "own/Shared",
            "data",
        ],
    );
    let fence = fence(
        &root,
        "own",
        &[
            ("own/Sub", false),
            ("own/p/q/Deep", false),
            ("own/Shared", true),
        ],
    );
    assert!(fence.own_shut);
    let own = root.join("own");
    let mut inside: Vec<(PathBuf, bool)> = fence.inside_own;
    inside.sort();
    assert_eq!(
        inside,
        [
            (own.join("Shared"), true),
            (own.join("Sub"), false),
            (own.join("p/q/Deep"), false),
        ]
    );
}

/// Only an invited matter inside the own one, or none: the own matter is re-opened whole.
#[test]
fn an_invited_matter_inside_the_own_one_leaves_it_open() {
    let (_dir, root) = tree("mfence-own-invited", &["own/Shared", "other", "data"]);
    let invited = fence(&root, "own", &[("own/Shared", true), ("other", false)]);
    assert!(!invited.own_shut);
    assert_eq!(invited.inside_own, [(root.join("own/Shared"), true)]);
    let plain = fence(&root, "own", &[("other", false)]);
    assert!(!plain.own_shut);
    assert_eq!(plain.inside_own, []);
}

/// A parent whose siblings of the own matter pass the launch's limit is left unfenced and
/// reported (Claude's settings file would outgrow what Claude Code reads); a later small matter
/// is still fenced.
#[test]
fn a_parent_past_the_limit_is_reported_unfenced() {
    let (_dir, root) = tree("mfence-unfenced", &["big/own", "small", "data"]);
    for n in 0..=super::MAX_PATHS {
        write::write_file(&root.join(format!("big/f{n}")), b"").unwrap();
    }
    let fence = fence(&root, "big/own", &[("big", false), ("small", false)]);
    assert_eq!(fence.unfenced, [fence.others[0].id]);
    assert_eq!(fence.others[0].paths, Vec::<PathBuf>::new());
    assert_eq!(fence.others[1].paths, [root.join("small")]);
}

/// For Codex an invited matter inside the own one stays denied (it could not be read-only); one
/// beside it is in no deny.
#[test]
fn codex_keeps_an_invited_matter_inside_the_own_one_denied() {
    let (_dir, root) = tree("mfence-codex", &["own/Inner", "beside", "fenced", "data"]);
    let fence = fence(
        &root,
        "own",
        &[("own/Inner", true), ("beside", true), ("fenced", false)],
    );
    let mut denied = fence.codex_denied();
    denied.sort();
    assert_eq!(denied, [root.join("fenced"), root.join("own/Inner")]);
}

/// The budget goes to the matters with the fewest paths first: one old parent with thousands
/// of entries can't push a newer plain sibling out; the parent is the one left unfenced.
#[test]
fn the_budget_fences_the_smallest_matters_first() {
    let (_dir, root) = tree("mfence-budget-order", &["big/own", "small", "data"]);
    for n in 0..super::MAX_PATHS {
        write::write_file(&root.join(format!("big/f{n}")), b"").unwrap();
    }
    let fence = fence(&root, "big/own", &[("big", false), ("small", false)]);
    assert_eq!(fence.unfenced, [fence.others[0].id]);
    assert_eq!(fence.others[1].paths, [root.join("small")]);
}

/// The own matter by its short form too: a failed read of `CLAUSE~1\…\Acme\Deal\missing.docx`,
/// the own matter inside the registered parent Acme, names no other matter (no offer).
#[test]
fn a_short_path_into_the_own_matter_names_no_parent() {
    let at = |path: &str| PathBuf::from(crate::testing::abs(path));
    // unix-paths-ok: `at` passes every path built from it through crate::testing::abs
    let long = "/Users/clauseo-lawyer/Matters/Acme";
    // unix-paths-ok: `at` passes every path built from it through crate::testing::abs
    let short = "/Users/CLAUSE~1/Matters/Acme";
    let mut acme = Other::new(id(1), "Acme".to_owned(), at(long), false);
    acme.spellings = vec![at(long), at(short)];
    let mut fence = MatterFence::asked(vec![acme]);
    fence.own = vec![at(&format!("{long}/Deal")), at(&format!("{short}/Deal"))];
    let missing = at(&format!("{short}/Deal/missing.docx"));
    assert!(innermost(&fence, &missing).is_none());
    let beside = at(&format!("{short}/Old/memo.docx"));
    assert_eq!(
        innermost(&fence, &beside).map(|other| other.id),
        Some(id(1))
    );
}

/// A matter registered through a link (`Linked` → `real/m`, outside every deny the sandbox has
/// otherwise): Claude Code's sandbox resolves no link in a deny, so both spellings are fenced,
/// the link's and where it leads; invited, both are allowed reads.
#[cfg(unix)]
#[test]
fn a_matter_registered_through_a_link_is_fenced_by_both_spellings() {
    let (_dir, root) = tree("mfence-link", &["own", "real/m", "data"]);
    write::symlink(&root.join("real/m"), &root.join("Linked")).unwrap();
    let both = [root.join("real/m"), root.join("Linked")];
    let fenced = fence(&root, "own", &[("Linked", false)]);
    let folders: Vec<&Path> = fenced.fenced_folders().collect();
    assert_eq!(folders, both);
    let invited = fence(&root, "own", &[("Linked", true)]);
    let allowed: Vec<&Path> = invited.read_allowed().collect();
    assert_eq!(allowed, both);
}
