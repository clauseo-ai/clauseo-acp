//! The menu bar's words: both languages for every key, Hindi in Devanagari, an ellipsis where
//! the item opens a window, and the review page saying the same words.

use clauseo_protocol::Language;

use super::{Key, text};

/// Whether `text` holds a Devanagari letter.
fn devanagari(text: &str) -> bool {
    text.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c))
}

#[test]
fn every_key_has_english_and_hindi() {
    for key in Key::ALL {
        let (en, hi) = (text(Language::En, key), text(Language::Hi, key));
        assert!(!en.trim().is_empty(), "{key:?}");
        assert!(devanagari(hi), "{key:?}: {hi}");
        assert!(!devanagari(en), "{key:?}: {en}");
        assert_eq!(en.trim(), en, "{key:?}");
        assert_eq!(hi.trim(), hi, "{key:?}");
        assert!(!hi.contains("  ") && !en.contains("  "), "{key:?}");
        // a menu item is a label: no sentence's full stop or danda
        assert!(!en.ends_with('.') && !hi.ends_with('।'), "{key:?}");
        assert_eq!(en.ends_with('…'), hi.ends_with('…'), "{key:?}");
    }
}

#[test]
fn the_app_keeps_its_name_in_both_languages() {
    for key in [Key::About, Key::Hide, Key::Quit] {
        assert!(text(Language::En, key).contains("Clauseo"), "{key:?}");
        assert!(text(Language::Hi, key).contains("Clauseo"), "{key:?}");
    }
}

#[test]
fn the_rooms_commands_use_the_rooms_words() {
    assert_eq!(text(Language::Hi, Key::NewConversation), "नई बातचीत");
    assert_eq!(text(Language::Hi, Key::NewMatter), "नया मामला");
    assert_eq!(text(Language::Hi, Key::Find), "खोजें");
    assert_eq!(text(Language::En, Key::Settings), "Settings…");
}

/// Every word here, in both languages, appears verbatim in its review home,
/// `ui/app/src/review/registry-mn.tsx`, under its id, so what the reviewers read is what Rust
/// draws.
#[test]
fn the_words_are_the_reviewed_copy() {
    let registry = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/app/src/review/registry-mn.tsx");
    let registry = std::fs::read_to_string(&registry).unwrap();
    for (n, key) in (1..).zip(Key::ALL) {
        let id = format!("MN-{n:02}");
        assert!(registry.contains(&format!("\"{id}\"")), "no {id}");
        for language in [Language::En, Language::Hi] {
            let quoted = format!("\"{}\"", text(language, key));
            assert!(registry.contains(&quoted), "{id} lacks {quoted}");
        }
    }
}
