//! The auto-deny for other matters: a request's path, search folder or shell word that reaches
//! another matter's fence path names that matter (the innermost), and the session's own matter
//! and a name that only starts with a fenced one name none.

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;
use serde_json::json;

use super::{named_in_words, other_matter_on};
use crate::agent::data_fence::resolve::Disk;
use crate::agent::data_fence::tests::{check, layout};
use crate::network_paths::{FOLD_CASE, NetEnv};

fn id(n: u128) -> MatterId {
    let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
    MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
}

/// `outside` as matter 1 and `outside/inner` as matter 2, canonical.
fn matters(root: &Path) -> Vec<(MatterId, Vec<PathBuf>)> {
    let outside = dunce::canonicalize(root.join("outside")).unwrap();
    crate::write::create_dir_all(&outside.join("inner")).unwrap();
    vec![
        (id(1), vec![outside.clone()]),
        (id(2), vec![outside.join("inner")]),
    ]
}

fn found(root: &Path, paths: &[&str], input: Option<&serde_json::Value>) -> Option<MatterId> {
    let matters = matters(root);
    let net = NetEnv::current();
    other_matter_on(
        &check(root, paths, input),
        &matters,
        FOLD_CASE,
        &Disk::real(&net),
    )
}

#[test]
fn a_path_in_another_matter_names_the_innermost() {
    let dir = layout("matters-check-path");
    let root = dir.path();
    let outside = root.join("outside");
    let top = outside.join("a.txt");
    let inner = outside.join("inner").join("b.txt");
    assert_eq!(found(root, &[top.to_str().unwrap()], None), Some(id(1)));
    assert_eq!(found(root, &[inner.to_str().unwrap()], None), Some(id(2)));
    // The session's own matter, by a relative path: none.
    assert_eq!(found(root, &["sub/a.txt"], None), None);
}

#[test]
fn a_search_folder_holding_another_matter_names_it() {
    let dir = layout("matters-check-root");
    let root = dir.path();
    let outside = root.join("outside");
    let input = json!({"command": format!("grep -r secret {}", outside.display())});
    assert_eq!(found(root, &[], Some(&input)), Some(id(1)));
}

#[test]
fn a_word_naming_a_fenced_path_counts_only_up_to_a_names_end() {
    let dir = layout("matters-check-words");
    let root = dir.path();
    let outside = dunce::canonicalize(root.join("outside")).unwrap();
    let matters = matters(root);
    let words = |word: String| super::named_in_words(&[word], &matters, FOLD_CASE);
    assert_eq!(words(format!("{}/x", outside.display())), Some(id(1)));
    assert_eq!(words(format!("{}/inner/y", outside.display())), Some(id(2)));
    assert_eq!(words(format!("{} 2/x", outside.display())), None);
}

/// A shell word names a fence path folded as the safety fold folds names, where names are
/// folded: `Zoë` written NFD names the matter stored NFC; a longer name still does not.
#[test]
fn a_word_in_another_unicode_form_names_the_matter() {
    let matters = [(id(1), vec![PathBuf::from("/m/Zo\u{eb}")])];
    let word = |text: &str| vec![text.to_owned()];
    let named = named_in_words(&word("cat /m/Zoe\u{308}/x"), &matters, true);
    assert_eq!(named, Some(id(1)));
    assert_eq!(
        named_in_words(&word("cat /m/Zoetrope/x"), &matters, true),
        None
    );
    assert_eq!(
        named_in_words(&word("cat /m/ZO\u{cb}"), &matters, true),
        Some(id(1))
    );
}
