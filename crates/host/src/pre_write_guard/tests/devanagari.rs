//! Devanagari names held open (decision 0007, Unicode names): the guard refuses a write that
//! names a held file in the other Unicode form, on every OS, since its match only refuses; a
//! file whose name is merely another Devanagari word still goes. Word's owner file in another
//! form than the document's holds it too.
//!
//! `ऱ` (U+0931) composes: NFD writes it `र` + nukta U+093C. `क़` (U+0958) is a composition
//! exclusion: NFC and NFD both write it `क` + U+093C.

use serde_json::json;

use super::backstop::{add, check};
use super::{Fixture, deny, guard_json};
use crate::clock::{Clock, SystemClock};

/// `ऱिपोर्ट.docx` composed; Word drops one character of its 7-character stem.
const REPORT_NFC: &str = "\u{931}\u{93f}\u{92a}\u{94b}\u{930}\u{94d}\u{91f}.docx";
const REPORT_OWNER: &str = "~$\u{93f}\u{92a}\u{94b}\u{930}\u{94d}\u{91f}.docx";
/// The same name decomposed.
const REPORT_NFD: &str = "\u{930}\u{93c}\u{93f}\u{92a}\u{94b}\u{930}\u{94d}\u{91f}.docx";
/// `क़ानून.docx` with the precomposed nukta letter; a stem short enough for Word to keep whole.
const LAW_PRECOMPOSED: &str = "\u{958}\u{93e}\u{928}\u{942}\u{928}.docx";
/// The same name decomposed, as NFC and NFD write it.
const LAW_DECOMPOSED: &str = "\u{915}\u{93c}\u{93e}\u{928}\u{942}\u{928}.docx";
/// `अनुबंध.docx`: another word, held by nothing.
const FREE: &str = "\u{905}\u{928}\u{941}\u{92c}\u{902}\u{927}.docx";

#[test]
fn a_held_devanagari_name_in_the_other_form_is_refused() {
    let fx = Fixture::new("dev-forms");
    add(&fx, &[REPORT_NFC, REPORT_OWNER]);
    for command in [
        format!("echo hi | tee {REPORT_NFD}"),
        format!("echo hi > {REPORT_NFC}"),
    ] {
        assert_eq!(check(&fx, "Bash", &command), deny(REPORT_NFC), "{command}");
    }
    assert_eq!(check(&fx, "Bash", &format!("echo hi > {FREE}")), "");
}

/// A nukta letter precomposed on one side and decomposed on the other, both ways round.
#[test]
fn a_held_nukta_name_in_the_other_form_is_refused() {
    for (n, (held, typed)) in [
        (LAW_PRECOMPOSED, LAW_DECOMPOSED),
        (LAW_DECOMPOSED, LAW_PRECOMPOSED),
    ]
    .into_iter()
    .enumerate()
    {
        let fx = Fixture::new(&format!("dev-nukta-{n}"));
        add(&fx, &[held, &format!("~${held}")]);
        let command = format!("cp x {typed}");
        assert_eq!(check(&fx, "Bash", &command), deny(held), "{command}");
        let write = json!({"file_path": typed, "content": "new"});
        assert_eq!(
            guard_json(&fx.hook("Write", &write)),
            deny(typed),
            "{typed:?}"
        );
        let free = json!({"file_path": FREE, "content": "new"});
        assert_eq!(guard_json(&fx.hook("Write", &free)), "");
    }
}

/// Word's owner file in the other form than the document's still holds it, for the file
/// tools and the shell alike.
#[test]
fn an_owner_file_in_the_other_form_holds_the_document() {
    let fx = Fixture::new("dev-owner");
    add(&fx, &[LAW_DECOMPOSED, &format!("~${LAW_PRECOMPOSED}")]);
    let write = json!({"file_path": LAW_DECOMPOSED, "content": "new"});
    assert_eq!(guard_json(&fx.hook("Write", &write)), deny(LAW_DECOMPOSED));
    let command = format!("echo hi > {LAW_DECOMPOSED}");
    assert_eq!(check(&fx, "Bash", &command), deny(LAW_DECOMPOSED));
}

/// A `?` wildcard counts characters, and composing changes the count: `??िपोर्ट.docx` matches
/// the decomposed `र` + nukta as stored, `?ानून.docx` the precomposed `क़`. Each is refused.
#[test]
fn a_wildcard_matching_the_held_name_as_stored_is_refused() {
    for (n, (held, owner, word)) in [
        (
            REPORT_NFD,
            format!("~${REPORT_NFD}"),
            "??\u{93f}\u{92a}\u{94b}\u{930}\u{94d}\u{91f}.docx",
        ),
        (
            LAW_PRECOMPOSED,
            format!("~${LAW_PRECOMPOSED}"),
            "?\u{93e}\u{928}\u{942}\u{928}.docx",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let fx = Fixture::new(&format!("dev-glob-{n}"));
        add(&fx, &[held, &owner]);
        let command = format!("cp x {word}");
        assert_eq!(check(&fx, "Bash", &command), deny(held), "{command}");
    }
}

/// One guard call over a folder of 5,000 Devanagari names beside one owner file lists the folder
/// once, not once per name: well under a second (it took 15.9 s when each name listed the folder
/// again).
#[test]
fn many_devanagari_names_beside_an_owner_file_cost_one_listing() {
    let fx = Fixture::new("dev-many");
    let names: Vec<String> = (0..5000).map(|n| format!("{FREE_STEM} {n}.docx")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    add(&fx, &refs);
    let hook = fx.hook("Bash", &json!({"command": "echo hi > new.txt"}));
    let clock = SystemClock;
    let started = clock.now().mono;
    assert_eq!(guard_json(&hook), "");
    let took = clock.now().mono.saturating_sub(started);
    eprintln!("the guard call over 5,000 Devanagari names took {took:?}");
    assert!(
        took < std::time::Duration::from_secs(1),
        "one guard call took {took:?}"
    );
}

/// `अनुबंध`.
const FREE_STEM: &str = "\u{905}\u{928}\u{941}\u{92c}\u{902}\u{927}";
