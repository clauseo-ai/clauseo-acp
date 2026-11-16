use serde_json::{Value, json};

use super::{Added, MAX_PENDING, Pending};

#[test]
fn the_cap_refuses_one_more_and_a_release_frees_a_slot() {
    let pending = Pending::default();
    for n in 0..MAX_PENDING {
        assert_eq!(pending.add(&json!(n)), Added::Pending, "{n}");
    }
    assert_eq!(pending.add(&json!("one more")), Added::Full);
    assert!(pending.release(&json!(3)));
    assert_eq!(pending.add(&json!("one more")), Added::Pending);
    assert_eq!(pending.add(&json!("and another")), Added::Full);
}

#[test]
fn a_release_finds_only_what_was_added_and_ids_of_different_types_differ() {
    let pending = Pending::default();
    assert_eq!(pending.add(&json!(1)), Added::Pending);
    assert!(
        !pending.release(&json!("1")),
        "a string id is not the number"
    );
    assert!(!pending.release(&json!(2)));
    assert!(pending.release(&json!(1)));
    assert!(!pending.release(&json!(1)), "answered once");
}

#[test]
fn a_repeated_id_counts_once_per_request_and_close_returns_each_and_refuses_later_ones() {
    let pending = Pending::default();
    assert_eq!(pending.add(&json!(5)), Added::Pending);
    assert_eq!(pending.add(&json!(5)), Added::Pending);
    assert_eq!(pending.add(&json!("x")), Added::Pending);
    assert!(pending.release(&json!(5)));
    let mut left: Vec<String> = pending.close().iter().map(ToString::to_string).collect();
    left.sort();
    assert_eq!(left, ["\"x\"", "5"]);
    assert_eq!(pending.close(), Vec::<Value>::new());
    assert_eq!(
        pending.add(&json!(9)),
        Added::Closed,
        "a closed set records nothing"
    );
    assert_eq!(pending.close(), Vec::<Value>::new());
}
