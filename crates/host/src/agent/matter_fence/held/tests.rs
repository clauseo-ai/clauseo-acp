//! Which matter on the list lies inside the session's own: by every spelling, case folded where
//! asked, never by a shared name prefix, never a forgotten one, invited or not.

use std::path::PathBuf;

use clauseo_protocol::{ErrorCode, MatterId};

use super::{held, held_as, recheck};
use crate::agent::matter_fence::{MatterFence, Other};
use crate::pipeline::tests::support::TempDir;
use crate::testing::abs;

fn id(n: u128) -> MatterId {
    let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
    MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
}

/// Matter `n` at `folder`, its one spelling the folder as given.
fn other(n: u128, folder: &str, invited: bool) -> Other {
    let folder = PathBuf::from(abs(folder));
    let mut other = Other::new(id(n), format!("M{n}"), folder.clone(), invited);
    other.spellings = vec![folder];
    other
}

fn own(folder: &str) -> Vec<PathBuf> {
    vec![PathBuf::from(abs(folder))]
}

#[test]
fn a_matter_inside_is_held_invited_or_not_and_a_prefix_or_the_parent_is_not() {
    let others = [
        other(1, "/p/Acme2", false),
        other(2, "/p", false),
        other(3, "/p/Acme", false),
        other(4, "/p/Acme/W/Sub", true),
    ];
    let found = held_as(&others, &own("/p/Acme"), false).map(|other| other.id);
    assert_eq!(found, Some(id(4)), "invited, still held");
    assert_eq!(held_as(&others[..3], &own("/p/Acme"), false), None);
    // The inner matter's conversation: the outer one holds it, it holds nothing.
    assert_eq!(held_as(&others[..3], &own("/p/Acme/W/Sub"), false), None);
}

#[test]
fn a_forgotten_matter_inside_is_not_held() {
    let mut forgotten = other(1, "/p/Acme/W/Sub", false);
    forgotten.listed = false;
    assert_eq!(held_as(&[forgotten], &own("/p/Acme"), false), None);
}

#[test]
fn a_case_variant_spelling_is_held_where_names_fold() {
    let others = [other(1, "/p/ACME/w/Sub", false)];
    assert_eq!(held_as(&others, &own("/p/Acme"), false), None);
    let found = held_as(&others, &own("/p/Acme"), true).map(|other| other.id);
    assert_eq!(found, Some(id(1)));
}

#[test]
fn any_spelling_inside_any_own_spelling_is_held() {
    let mut linked = other(1, "/q/Link/Sub", false);
    linked.spellings.push(PathBuf::from(abs("/p/Acme/W/Sub")));
    let mut fence = MatterFence::asked(vec![linked]);
    fence.own = vec![PathBuf::from(abs("/r/Acme")), PathBuf::from(abs("/p/Acme"))];
    assert_eq!(held(&fence).map(|other| other.id), Some(id(1)));
}

#[cfg(unix)]
#[test]
fn the_recheck_follows_a_link_moved_since_the_launch() {
    let dir = TempDir::new("held-recheck");
    let root = dunce::canonicalize(dir.path()).unwrap();
    for name in ["Acme/W/Sub", "Elsewhere/Sub"] {
        crate::write::create_dir_all(&root.join(name)).unwrap();
    }
    let link = root.join("Link");
    std::os::unix::fs::symlink(root.join("Elsewhere"), &link).unwrap();
    let asked = Other::new(id(1), "Sub".into(), link.join("Sub"), false);
    let fence = MatterFence::asked(vec![asked]);
    let stored = root.join("Acme");
    assert!(recheck(&fence, &stored).is_ok(), "elsewhere");
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(root.join("Acme/W"), &link).unwrap();
    let refused = recheck(&fence, &stored).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::MatterHoldsOtherMatter);
    assert_eq!(refused.to_wire().matter(), Some(id(1)));
}
