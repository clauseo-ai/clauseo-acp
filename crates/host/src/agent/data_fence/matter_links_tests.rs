//! The matter's link walk (`matter_links.rs`): a link out of the matter, a broken one, a walk
//! past its cap or a matter that can't be listed leave the matter without read allow rules; a
//! link that stays inside, or none, keeps them.

use super::{links_out, readable_matter};
use crate::pipeline::tests::support::TempDir;

/// A matter under `dir` with a file and a sub-folder, and a folder beside it.
fn matter(dir: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let matter = dir.path().join("Smith v Jones (2024)");
    let outside = dir.path().join("data");
    for folder in [matter.join("sub"), outside.clone()] {
        crate::write::create_dir_all(&folder).unwrap();
    }
    crate::write::write_file(&matter.join("sub").join("memo.txt"), b"memo").unwrap();
    crate::write::write_file(&outside.join("settings.json"), b"{}").unwrap();
    (dunce::canonicalize(&matter).unwrap(), outside)
}

/// A plain matter keeps its rules, in both spellings when they differ.
#[test]
fn a_matter_with_no_link_keeps_its_spellings() {
    let dir = TempDir::new("matter-links-none");
    let (real, _) = matter(&dir);
    assert_eq!(links_out(&real, 100), Some(false));
    assert_eq!(readable_matter(&real, &real), std::slice::from_ref(&real));
    let stored = dir.path().join("stored");
    assert_eq!(readable_matter(&real, &stored), [real, stored]);
}

/// A matter that can't be listed, or one with more entries than the cap, gets none.
#[test]
fn a_matter_past_the_cap_or_unlistable_gets_no_rules() {
    let dir = TempDir::new("matter-links-cap");
    let (real, _) = matter(&dir);
    for n in 0..5 {
        crate::write::write_file(&real.join(format!("f{n}.txt")), b"x").unwrap();
    }
    assert_eq!(links_out(&real, 4), None);
    assert_eq!(links_out(&real, 100), Some(false));
    let gone = dir.path().join("gone");
    assert_eq!(links_out(&gone, 100), None);
    assert_eq!(
        readable_matter(&gone, &gone),
        Vec::<std::path::PathBuf>::new()
    );
}

/// A symbolic link out of the matter (to a folder or a file, at any depth) or a broken one
/// leaves it without rules; one that stays inside does not (the review's H2).
#[cfg(unix)]
#[test]
fn a_symbolic_link_out_of_the_matter_leaves_it_without_rules() {
    for (name, out) in [
        ("dir out", true),
        ("file out", true),
        ("broken", true),
        ("inside", false),
    ] {
        let dir = TempDir::new("matter-links-unix");
        let (real, outside) = matter(&dir);
        let (target, link) = match name {
            "dir out" => (outside, real.join("link")),
            "file out" => (outside.join("settings.json"), real.join("sub").join("f")),
            "broken" => (real.join("nothing"), real.join("sub").join("dangling")),
            _ => (real.join("sub"), real.join("again")),
        };
        crate::write::symlink(&target, &link).unwrap();
        assert_eq!(links_out(&real, 100), Some(out), "{name}");
        assert_eq!(readable_matter(&real, &real).is_empty(), out, "{name}");
    }
}

/// A junction out of the matter leaves it without rules.
#[cfg(windows)]
#[test]
fn a_junction_out_of_the_matter_leaves_it_without_rules() {
    let dir = TempDir::new("matter-links-junction");
    let (real, outside) = matter(&dir);
    clauseo_sandbox::windows::test_junction::junction(&real.join("link"), &outside).unwrap();
    assert_eq!(links_out(&real, 100), Some(true));
    assert_eq!(
        readable_matter(&real, &real),
        Vec::<std::path::PathBuf>::new()
    );
}
