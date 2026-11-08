//! The confirm's text: bidi, format and control characters escaped, letters kept.

use clauseo_protocol::Language;

use super::{escaped, folder_words};

/// Each case and how the confirm shows it.
#[test]
fn the_confirm_escapes_bidi_and_controls_and_keeps_letters() {
    for (path, shown) in [
        // RLO turns "fdp.exe" around on screen.
        ("C:\\Cases\\\u{202E}exe.pdf", "C:\\Cases\\<U+202E>exe.pdf"),
        ("C:\\\u{2066}Acme\u{2069}", "C:\\<U+2066>Acme<U+2069>"),
        (
            "C:\\a\u{200E}b\u{200F}c\u{061C}",
            "C:\\a<U+200E>b<U+200F>c<U+061C>",
        ),
        (
            "C:\\zero\u{200B}width\u{FEFF}",
            "C:\\zero<U+200B>width<U+FEFF>",
        ),
        ("C:\\Acme\nD:\\Other", "C:\\Acme<U+000A>D:\\Other"),
        ("C:\\a\tb\u{7F}\u{85}", "C:\\a<U+0009>b<U+007F><U+0085>"),
        // ZWJ and ZWNJ write Devanagari conjuncts: kept.
        ("C:\\क्\u{200D}ष", "C:\\क्\u{200D}ष"),
        ("C:\\क्\u{200C}ष", "C:\\क्\u{200C}ष"),
        ("C:\\मुकदमे\\शर्मा बनाम वर्मा", "C:\\मुकदमे\\शर्मा बनाम वर्मा"),
        (
            "C:\\Cases\\Smith v Jones (2024)",
            "C:\\Cases\\Smith v Jones (2024)",
        ),
        ("D:\\Müller & Søn\\日本", "D:\\Müller & Søn\\日本"),
    ] {
        assert_eq!(escaped(path), shown, "{path:?}");
    }
}

/// The confirm speaks the app's language, the path below its title either way.
#[test]
fn the_confirm_follows_the_language() {
    let path = "C:\\Cases\\मेहरा \u{202E}";
    let hi = folder_words(path, Language::Hi);
    assert_eq!(hi.title, "मामले के लिए यह फ़ोल्डर इस्तेमाल करें?");
    assert_eq!(
        hi.message,
        format!("{}\n\nC:\\Cases\\मेहरा <U+202E>", hi.title)
    );
    assert_eq!(
        (hi.ok.as_str(), hi.cancel.as_str()),
        ("यह फ़ोल्डर इस्तेमाल करें", "रद्द करें")
    );
    let en = folder_words(path, Language::En);
    assert_eq!(en.title, "Use this folder for the matter?");
    assert_eq!(
        (en.ok.as_str(), en.cancel.as_str()),
        ("Use this folder", "Cancel")
    );
}
