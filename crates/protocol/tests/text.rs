//! Text costs: `escaped_len`, `utf16_len`, `TextOrBlob::inline_len`, previews and blob refs.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

use clauseo_protocol::{
    BlobHash, BlobRef, Count, MAX_SAFE, PREVIEW_BYTES, TextOrBlob, escaped_len, preview_of,
    utf16_len,
};
use proptest::prelude::*;

/// Strings rich in what changes the counts: control characters, quotes, backslashes, DEL,
/// 2-, 3- and 4-byte characters, and plain ASCII.
fn tricky_text() -> impl Strategy<Value = String> {
    let tricky_char = prop_oneof![
        proptest::char::range('\0', '\u{1f}'),
        Just('"'),
        Just('\\'),
        Just('\u{7f}'),
        Just('é'),
        Just('€'),
        Just('😀'),
        proptest::char::range(' ', '~'),
        any::<char>(),
    ];
    proptest::collection::vec(tricky_char, 0..80).prop_map(String::from_iter)
}

#[test]
fn escaped_len_counts_json_escapes() {
    let cases = [
        ("", 0),
        ("plain", 5),
        ("\"", 2),
        ("\\", 2),
        ("\n\r\t\u{8}\u{c}", 10),
        ("\u{0}", 6),
        ("\u{1f}", 6),
        ("\u{7f}", 1),
        ("é", 2),
        ("😀", 4),
        ("say \"hi\"\n", 12),
        // Escapes on both sides of an 8-byte boundary, for the word-at-a-time scan.
        ("abcdefg\"h\u{1}", 16),
    ];
    for (text, expected) in cases {
        assert_eq!(escaped_len(text), expected, "{text:?}");
        let json = serde_json::to_string(text).unwrap();
        assert_eq!(json.len() - 2, expected, "serde_json disagrees on {text:?}");
    }
}

proptest! {
    #[test]
    fn escaped_len_matches_serde_json(text in tricky_text()) {
        let json = serde_json::to_string(&text).unwrap();
        prop_assert_eq!(escaped_len(&text), json.len() - 2);
    }

    #[test]
    fn utf16_len_matches_encode_utf16(text in tricky_text()) {
        prop_assert_eq!(utf16_len(&text), text.encode_utf16().count());
    }
}

#[test]
fn utf16_len_adds_up() {
    assert_eq!(utf16_len(""), 0);
    assert_eq!(utf16_len("a"), 1);
    assert_eq!(utf16_len("é"), 1);
    assert_eq!(utf16_len("€"), 1);
    assert_eq!(utf16_len("😀"), 2);
    assert_eq!(utf16_len("a é € 😀"), 1 + 1 + 1 + 1 + 1 + 1 + 2);
    assert_eq!(utf16_len("😀😀"), 4);
}

#[test]
fn inline_len_counts_inline_text_only() {
    assert_eq!(TextOrBlob::Inline("a\"b".to_owned()).inline_len(), 4);
    let blob = BlobRef::new(
        BlobHash::from_bytes([7; 32]),
        Count::from(10),
        "\"\"\"".to_owned(),
    )
    .unwrap();
    assert_eq!(TextOrBlob::Blob(blob).inline_len(), 0);
}

#[test]
fn text_or_blob_json_shape() {
    let inline = TextOrBlob::Inline("hi".to_owned());
    assert_eq!(
        serde_json::to_string(&inline).unwrap(),
        r#"{"inline":"hi"}"#
    );
    let blob = TextOrBlob::Blob(
        BlobRef::new(
            BlobHash::from_bytes([0xab; 32]),
            Count::from(3),
            "h".to_owned(),
        )
        .unwrap(),
    );
    let json = serde_json::to_string(&blob).unwrap();
    let hash = "ab".repeat(32);
    assert_eq!(
        json,
        format!(r#"{{"blob":{{"hash":"{hash}","size_bytes":3,"preview":"h"}}}}"#)
    );
    assert_eq!(serde_json::from_str::<TextOrBlob>(&json).unwrap(), blob);
    let packed = rmp_serde::to_vec_named(&blob).unwrap();
    assert_eq!(rmp_serde::from_slice::<TextOrBlob>(&packed).unwrap(), blob);
}

#[test]
fn blob_ref_rejects_large_values() {
    let hash = BlobHash::from_bytes([1; 32]);
    let error = Count::new(1 << 53).unwrap_err().to_string();
    assert!(error.contains("is outside 0..=9007199254740991"), "{error}");
    let error = BlobRef::new(hash, Count::ZERO, "x".repeat(PREVIEW_BYTES + 1))
        .unwrap_err()
        .to_string();
    assert!(error.contains("longer than 512 bytes"), "{error}");
    // A preview is a start of the blob's text, so it is no longer than the blob.
    let error = BlobRef::new(hash, Count::from(3), "abcd".to_owned())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("longer than the blob's size_bytes"),
        "{error}"
    );
    assert!(BlobRef::new(hash, Count::from(3), "abc".to_owned()).is_ok());
    assert!(BlobRef::new(hash, Count::ZERO, String::new()).is_ok());
    let refused = serde_json::from_str::<BlobRef>(&format!(
        r#"{{"hash":"{}","size_bytes":3,"preview":"abcd"}}"#,
        "01".repeat(32)
    ));
    assert!(refused.unwrap_err().to_string().contains("size_bytes"));
    let max = Count::new(MAX_SAFE).unwrap();
    let at_limit = BlobRef::new(hash, max, "x".repeat(PREVIEW_BYTES)).unwrap();
    assert_eq!(at_limit.hash(), hash);
    assert_eq!(at_limit.size_bytes().get(), MAX_SAFE);
    assert_eq!(at_limit.preview().len(), PREVIEW_BYTES);
    let hex = "01".repeat(32);
    let too_big = format!(r#"{{"hash":"{hex}","size_bytes":9007199254740992,"preview":""}}"#);
    let error = serde_json::from_str::<BlobRef>(&too_big)
        .unwrap_err()
        .to_string();
    assert!(error.contains("is outside 0..=9007199254740991"), "{error}");
    let upper = format!(
        r#"{{"hash":"{}","size_bytes":1,"preview":""}}"#,
        "AB".repeat(32)
    );
    let error = serde_json::from_str::<BlobRef>(&upper)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("expected 64 lowercase hex characters"),
        "{error}"
    );
}

#[test]
fn preview_cuts_on_a_char_boundary() {
    assert_eq!(preview_of("short"), "short");
    let long = "é".repeat(400); // 800 bytes; 512 falls between the bytes of no char.
    assert_eq!(preview_of(&long).len(), 512);
    let odd = format!("a{}", "é".repeat(400)); // 512 falls inside an é.
    assert_eq!(preview_of(&odd).len(), 511);
}
