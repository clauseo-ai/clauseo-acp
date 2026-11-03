//! The one normalisation of the text the index stores and of the query text: Unicode NFC, and
//! the zero-width joiners removed (decision 0043). The stored text and the query meet in the
//! same form, so a word typed without a joiner finds the stored word with one, and a nukta
//! letter found in either of its encodings.

use unicode_normalization::UnicodeNormalization;

/// `text` in NFC, without ZERO WIDTH NON-JOINER (U+200C) and ZERO WIDTH JOINER (U+200D). The
/// joiners are removed first: a joiner between a base and a mark would otherwise block the
/// composition that NFC makes once it is gone.
pub(crate) fn normalise(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{200C}' | '\u{200D}'))
        .nfc()
        .collect()
}
