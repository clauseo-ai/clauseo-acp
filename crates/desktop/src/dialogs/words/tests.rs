//! The confirms' words: both languages for every key, the same slots in each, Hindi in
//! Devanagari, the English in the app's voice, and the review page saying the same words.

use std::collections::BTreeSet;

use clauseo_protocol::Language;

use super::{ConfirmWords, Key, OFF_VOICE, fill, text};

/// The `{slot}` names in `text`.
fn slots(text: &str) -> BTreeSet<&str> {
    text.split('{')
        .skip(1)
        .filter_map(|part| part.split_once('}').map(|(slot, _)| slot))
        .collect()
}

/// Whether `text` holds a Devanagari letter.
fn devanagari(text: &str) -> bool {
    text.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c))
}

#[test]
fn every_key_has_english_and_hindi_with_the_same_slots() {
    for key in Key::ALL {
        let (en, hi) = (text(Language::En, key), text(Language::Hi, key));
        assert!(!en.trim().is_empty(), "{key:?}");
        assert!(devanagari(hi), "{key:?}: {hi}");
        assert!(!devanagari(en), "{key:?}: {en}");
        assert_eq!(slots(en), slots(hi), "{key:?}");
        assert_eq!(en.trim(), en, "{key:?}");
        assert_eq!(hi.trim(), hi, "{key:?}");
        assert!(!hi.contains("  ") && !en.contains("  "), "{key:?}");
    }
}

#[test]
fn the_english_keeps_the_apps_voice() {
    for key in Key::ALL {
        let en = text(Language::En, key).to_lowercase();
        for word in OFF_VOICE {
            assert!(!en.contains(word), "{key:?}: {en}");
        }
    }
}

#[test]
fn a_sentence_ends_in_a_danda_and_a_question_stays_a_question() {
    for key in Key::ALL {
        let (en, hi) = (text(Language::En, key), text(Language::Hi, key));
        if en.ends_with('.') {
            assert!(hi.ends_with('।'), "{key:?}: {hi}");
        }
        assert_eq!(en.ends_with('?'), hi.ends_with('?'), "{key:?}");
    }
}

#[test]
fn fill_replaces_named_slots_once_and_leaves_the_rest() {
    let slots = [("name", "Claude"), ("url", "https://a.example/{name}")];
    assert_eq!(
        fill("{name} to {url}.", &slots),
        "Claude to https://a.example/{name}."
    );
    assert_eq!(fill("{other} {name", &slots), "{other} {name");
    assert_eq!(fill("कोई {name} नहीं।", &slots), "कोई Claude नहीं।");
}

#[test]
fn hindi_picks_the_hindi_words() {
    let keys = [Key::SwitchTitle, Key::SwitchMessage, Key::SwitchOk];
    let hi = ConfirmWords::new(Language::Hi, keys, &[("name", "ChatGPT")]);
    assert_eq!(hi.title, "किसी दूसरे खाते से साइन इन करें?");
    assert!(hi.message.contains("ChatGPT") && devanagari(&hi.message));
    assert_eq!(hi.cancel, "रद्द करें");
    let en = ConfirmWords::new(Language::En, keys, &[("name", "ChatGPT")]);
    assert_eq!(en.ok, "Sign out and continue");
    assert_eq!(en.cancel, "Cancel");
}

/// Every word here, in both languages, appears verbatim in its review home,
/// `ui/app/src/review/registry-nd.tsx`, under its id, so what the reviewers read is what Rust
/// draws.
#[test]
fn the_words_are_the_reviewed_copy() {
    let registry = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/app/src/review/registry-nd.tsx");
    let registry = std::fs::read_to_string(&registry).unwrap();
    for (n, key) in (1..).zip(Key::ALL) {
        let id = format!("ND-{n:02}");
        assert!(registry.contains(&format!("\"{id}\"")), "no {id}");
        for language in [Language::En, Language::Hi] {
            let quoted = format!("\"{}\"", text(language, key));
            assert!(registry.contains(&quoted), "{id} lacks {quoted}");
        }
    }
}
