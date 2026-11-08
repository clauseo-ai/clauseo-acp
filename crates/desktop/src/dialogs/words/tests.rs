//! The confirms' words: both languages for every key, the same slots in each, Hindi in
//! Devanagari, the English in the app's voice, and the review page saying the same words.

use std::collections::BTreeSet;

use clauseo_protocol::Language;

use super::{ConfirmWords, Key, OFF_VOICE, PENDING_HINDI, fill, isolate, text};

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

/// The keys whose Hindi is written: every key but [`PENDING_HINDI`]'s.
fn written_in_hindi() -> impl Iterator<Item = Key> {
    Key::ALL
        .into_iter()
        .filter(|key| !PENDING_HINDI.contains(key))
}

#[test]
fn every_key_has_english_and_hindi_with_the_same_slots() {
    for key in written_in_hindi() {
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
    for key in written_in_hindi() {
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

/// The review id of the key at `index` in [`Key::ALL`]: ND-01 to ND-15, then ND-22 on, since
/// ND-16 to ND-21 are the dialog shown when `WebView2` is missing (`webview2/words.rs`).
fn review_id(index: usize) -> String {
    let n = if index < 15 { index + 1 } else { index + 7 };
    format!("ND-{n:02}")
}

/// Every word here, in both languages, appears verbatim in its review home,
/// `ui/app/src/review/registry-nd.tsx`, under its id, so what the reviewers read is what Rust
/// draws.
#[test]
fn the_words_are_the_reviewed_copy() {
    let registry = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/app/src/review/registry-nd.tsx");
    let registry = std::fs::read_to_string(&registry).unwrap();
    let approvals = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/app/copy-approvals/ND.json");
    let approvals: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&approvals).unwrap()).unwrap();
    assert_eq!(review_id(14), "ND-15");
    assert_eq!(review_id(15), "ND-22");
    for (index, key) in Key::ALL.into_iter().enumerate() {
        if PENDING_HINDI.contains(&key) {
            continue;
        }
        let id = review_id(index);
        let approved = &approvals[&id];
        assert_eq!(approved["en"], text(Language::En, key), "{id} in ND.json");
        assert_eq!(approved["hi"], text(Language::Hi, key), "{id} in ND.json");
        assert!(registry.contains(&format!("\"{id}\"")), "no {id}");
        for language in [Language::En, Language::Hi] {
            let quoted = format!("\"{}\"", text(language, key));
            assert!(registry.contains(&quoted), "{id} lacks {quoted}");
        }
    }
}

/// The keys not yet reviewed: each has English, and its Hindi is either Devanagari or listed in
/// `PENDING_HINDI`, where English stands in until the app thread writes it. Each pending key is
/// listed once, after the reviewed keys (so the review ids of those stay as they are).
#[test]
fn a_key_without_hindi_is_pending_and_still_has_english() {
    for key in Key::ALL {
        let (en, hi) = (text(Language::En, key), text(Language::Hi, key));
        assert!(!en.trim().is_empty(), "{key:?}");
        assert_eq!(en.trim(), en, "{key:?}");
        assert!(!devanagari(en), "{key:?}: {en}");
        assert!(
            devanagari(hi) || PENDING_HINDI.contains(&key),
            "{key:?}: {hi}"
        );
        if PENDING_HINDI.contains(&key) {
            assert!(hi == en || devanagari(hi), "{key:?}: {hi}");
        }
    }
    for (n, key) in PENDING_HINDI.iter().enumerate() {
        assert_eq!(
            PENDING_HINDI.iter().filter(|k| *k == key).count(),
            1,
            "{key:?}"
        );
        let at = Key::ALL.iter().position(|k| k == key).unwrap();
        assert_eq!(at, Key::ALL.len() - PENDING_HINDI.len() + n, "{key:?}");
    }
}

/// A slot's value is isolated (FSI ... PDI) and loses its own bidi controls and line breaks,
/// so it can neither reorder the words around it nor close the isolate early.
#[test]
fn isolate_wraps_and_strips_what_could_break_out() {
    assert_eq!(isolate("Brown"), "\u{2068}Brown\u{2069}");
    assert_eq!(isolate("قضية"), "\u{2068}قضية\u{2069}");
    let hostile = "A\u{2069}\u{202E}B\u{200F}\nC\u{2066}";
    assert_eq!(isolate(hostile), "\u{2068}ABC\u{2069}");
    assert_eq!(isolate(""), "\u{2068}\u{2069}");
}
