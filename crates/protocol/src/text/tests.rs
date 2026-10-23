//! The word tests behind `escaped_len`. `has_zero_byte` and `needs_escape` are exact, so each
//! agrees with a byte-by-byte check on every byte value in every lane of the word.

use super::{has_zero_byte, needs_escape};

/// Bytes that fill the lanes around the one under test: a clean byte, NUL, a quote, a backslash,
/// DEL, a high-bit byte, 0xFF and a space, so no filler hides a wrong answer.
const FILLERS: [u8; 8] = [b'A', 0x00, b'"', b'\\', 0x7F, 0x80, 0xFF, b' '];

/// Whether a byte needs escaping in JSON: below 0x20, `"` or `\`.
fn needs_escape_by_hand(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .any(|&byte| byte < 0x20 || byte == b'"' || byte == b'\\')
}

/// Checks both word tests on the word of `bytes`, lane 0 first, as `escaped_len_swar` reads it.
fn check(bytes: [u8; 8]) {
    let word = u64::from_ne_bytes(bytes);
    assert_eq!(has_zero_byte(word), bytes.contains(&0), "{bytes:?}");
    assert_eq!(
        needs_escape(word),
        needs_escape_by_hand(&bytes),
        "{bytes:?}"
    );
}

#[test]
fn word_tests_match_a_byte_by_byte_check_in_every_lane() {
    for lane in 0..8 {
        for byte in 0..=u8::MAX {
            for filler in FILLERS {
                let mut bytes = [filler; 8];
                bytes[lane] = byte;
                check(bytes);
            }
        }
    }
}

#[test]
fn word_tests_match_a_byte_by_byte_check_on_random_words() {
    // xorshift64 from a fixed seed: the same words on every run, so a failure reproduces.
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..100_000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        check(state.to_ne_bytes());
    }
}
