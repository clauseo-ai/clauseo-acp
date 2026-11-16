//! The other matters' rule, per agent and platform: the fenced one (Claude everywhere, Codex on
//! macOS and Linux) may try a file the lawyer asked for and points to the button Clauseo shows;
//! Codex on Windows, which nothing fences, knows a matter by its `.clauseo` folder, opens only
//! those a Clauseo note names (its rule names no matter), never on a chat request or a document,
//! and is told the menu item that lets it read one, in the app's words.

use clauseo_profile::instructions::{
    LEGAL, OTHER_MATTERS, invite_item, other_matters_unfenced, reads_note,
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

/// Codex on Windows: a fixed rule naming no matter; the note carries the grants.
#[test]
fn the_rule_for_codex_on_windows() {
    insta::assert_snapshot!(other_matters_unfenced());
    assert!(other_matters_unfenced().contains("even the lawyer's"));
}

/// The note a resumed Codex thread gets first in the lawyer's next message: the matters by name
/// and folder; with none, it says so.
#[test]
fn the_reads_note() {
    insta::assert_snapshot!(reads_note(&[(
        "Jones \"Appeal\"",
        r"C:\Users\l\Clients\Jones"
    )]));
    assert_eq!(
        reads_note(&[]),
        "Clauseo: from now on this conversation may read no other matter."
    );
}

/// The menu item is the app's: read from its copy entries, and a key of its words table.
#[test]
fn the_menu_item_is_the_apps_own_words() {
    let item = invite_item();
    assert_ne!(item, "");
    let table = include_str!("../../../../ui/components/src/lib/words-triage-late.hi.ts");
    assert!(table.contains(&format!("\"{item}\":")), "{item}");
}
