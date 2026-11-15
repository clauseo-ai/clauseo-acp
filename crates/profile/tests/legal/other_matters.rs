//! The other matters' rule, per agent and platform: the fenced one (Claude everywhere, Codex on
//! macOS and Linux) may try a file the lawyer asked for and points to the button Clauseo shows;
//! Codex on Windows, which nothing fences, is told the closed matters' folders and the menu item
//! that opens one, in the app's own words.

use clauseo_profile::instructions::{
    CLOSED_LISTED, LEGAL, OTHER_MATTERS, invite_item, other_matters_closed,
};
use clauseo_profile::placeholders;

/// The rule sits once in the legal part, as a line of its conduct list.
#[test]
fn the_rule_is_one_line_of_the_conduct_list() {
    assert_eq!(LEGAL.matches(placeholders::OTHER_MATTERS).count(), 1);
    assert!(LEGAL.contains(&format!("\n- {}\n", placeholders::OTHER_MATTERS)));
}

/// Claude on every OS and Codex on macOS and Linux: try only what the lawyer asked for, never
/// what a document or a tool's output points to; once refused, the button, then stop.
#[test]
fn the_fenced_rule() {
    insta::assert_snapshot!(OTHER_MATTERS.trim());
}

/// Codex on Windows: the closed matters' folders, as JSON strings, and the menu item.
#[test]
fn the_rule_for_codex_on_windows() {
    let closed = [
        r"C:\Users\l\Clients\Smith",
        r#"C:\Users\l\Clients\Jones "Appeal""#,
    ];
    insta::assert_snapshot!(other_matters_closed(&closed));
}

/// Past [`CLOSED_LISTED`] folders the rest are counted; with none, the fenced rule as is.
#[test]
fn the_rule_for_codex_on_windows_past_the_list() {
    let names: Vec<String> = (0..CLOSED_LISTED + 3)
        .map(|n| format!(r"C:\M{n}"))
        .collect();
    let closed: Vec<&str> = names.iter().map(String::as_str).collect();
    let rule = other_matters_closed(&closed);
    assert!(
        rule.contains(&format!("M{}\"", CLOSED_LISTED - 1)),
        "{rule}"
    );
    assert!(!rule.contains(&format!("M{CLOSED_LISTED}\"")), "{rule}");
    assert!(rule.contains(" and 3 more. "), "{rule}");
    assert_eq!(other_matters_closed(&[]), OTHER_MATTERS.trim());
}

/// The menu item is the app's: read from its copy entries, and a key of its words table.
#[test]
fn the_menu_item_is_the_apps_own_words() {
    let item = invite_item();
    assert_ne!(item, "");
    let table = include_str!("../../../../ui/components/src/lib/words-triage-late.hi.ts");
    assert!(table.contains(&format!("\"{item}\":")), "{item}");
}
