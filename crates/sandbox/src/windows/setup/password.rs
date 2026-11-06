//! The pool users' passwords: [`PASSWORD_LEN`] characters drawn uniformly from [`ALPHABET`]
//! with OS randomness (`getrandom`), redrawn until all four classes (upper case, lower case,
//! digit, symbol) appear, so any Windows complexity policy up to 64 characters accepts it. No one
//! types these: the setup exe protects them for the owner (`secrets.bin`) and the runner reads
//! them back.
//!
//! Pure apart from `getrandom`: built on every OS so its tests run everywhere.

use zeroize::Zeroizing;

use super::Password;

/// How many characters a pool user's password has.
pub const PASSWORD_LEN: usize = 64;

/// Every character a password may hold: 26 upper case, 26 lower case, 10 digits and 27 symbols.
/// No space, quote, backslash or backtick, so the text needs no escaping anywhere.
pub const ALPHABET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$%&()*+,-./:;<=>?@[]^_{}~";

/// How many full draws are tried before giving up. One draw misses a class with a chance of
/// about 1 in 3,000,000,000 (lower case, the rarest class bar digits, at 26 of 89; digits miss
/// with about 1 in 40,000), so only a broken source reaches the limit.
const DRAWS: usize = 32;

/// A random byte below this is used, as `byte % ALPHABET.len()`; one at or above it is dropped,
/// so every character is equally likely.
const ACCEPT_BELOW: usize = 256 - 256 % ALPHABET.len();

/// A new password from the OS's random source. `None` when the source fails, or (never in
/// practice) when [`DRAWS`] draws all miss a class.
pub fn generate() -> Option<Password> {
    password_from(|bytes| getrandom::fill(bytes).is_ok())
}

/// A password from `fill`, which fills a buffer with random bytes and says whether it could.
/// `None` when `fill` fails or [`DRAWS`] draws all miss a class. Every buffer it fills is wiped.
pub fn password_from(mut fill: impl FnMut(&mut [u8]) -> bool) -> Option<Password> {
    let mut bytes = Zeroizing::new([0u8; 2 * PASSWORD_LEN]);
    for _draw in 0..DRAWS {
        // Sized up front: a String that grows leaves copies of its old buffer behind.
        let mut password = Zeroizing::new(String::with_capacity(PASSWORD_LEN));
        while password.len() < PASSWORD_LEN {
            if !fill(&mut bytes[..]) {
                return None;
            }
            for byte in bytes.iter().map(|byte| usize::from(*byte)) {
                if password.len() == PASSWORD_LEN {
                    break;
                }
                if byte < ACCEPT_BELOW
                    && let Some(chosen) = ALPHABET.get(byte % ALPHABET.len())
                {
                    password.push(char::from(*chosen));
                }
            }
        }
        if is_strong(&password) {
            return Some(password);
        }
    }
    None
}

/// Whether `password` is one [`generate`] could give: exactly [`PASSWORD_LEN`] characters, all
/// from [`ALPHABET`], with at least one of each class. The account calls refuse any other.
pub fn is_strong(password: &str) -> bool {
    let bytes = password.as_bytes();
    bytes.len() == PASSWORD_LEN
        && bytes.iter().all(|byte| ALPHABET.contains(byte))
        && bytes.iter().any(u8::is_ascii_uppercase)
        && bytes.iter().any(u8::is_ascii_lowercase)
        && bytes.iter().any(u8::is_ascii_digit)
        && bytes.iter().any(|byte| !byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn the_alphabet_has_four_classes_and_nothing_to_escape() {
        assert_eq!(ALPHABET.len(), 89);
        assert_eq!(ALPHABET.iter().collect::<BTreeSet<_>>().len(), 89);
        let count = |class: fn(&u8) -> bool| ALPHABET.iter().filter(|byte| class(byte)).count();
        assert_eq!(count(u8::is_ascii_uppercase), 26);
        assert_eq!(count(u8::is_ascii_lowercase), 26);
        assert_eq!(count(u8::is_ascii_digit), 10);
        assert_eq!(count(u8::is_ascii_punctuation), 27);
        for awkward in b" \"'\\`" {
            assert!(!ALPHABET.contains(awkward), "{}", char::from(*awkward));
        }
        assert_eq!(ACCEPT_BELOW % ALPHABET.len(), 0);
    }

    #[test]
    fn a_thousand_draws_are_each_64_characters_of_all_four_classes() {
        let mut seen = BTreeSet::new();
        for _ in 0..1_000 {
            let password = generate().unwrap();
            assert_eq!(password.len(), PASSWORD_LEN);
            assert_eq!(password.chars().count(), PASSWORD_LEN);
            assert!(is_strong(&password));
            assert!(seen.insert((*password).clone()), "a password came twice");
        }
        // Every character of the alphabet turns up over 64,000 draws of one in 89.
        let used: BTreeSet<u8> = seen.iter().flat_map(|text| text.bytes()).collect();
        assert_eq!(used.len(), ALPHABET.len());
    }

    #[test]
    fn bytes_past_the_last_whole_alphabet_are_dropped() {
        // 178 and above would favour the first 78 characters; the source gives them first.
        let mut next = 177u8;
        let mut calls = 0;
        let password = password_from(|bytes| {
            calls += 1;
            for byte in bytes.iter_mut() {
                next = if next == u8::MAX { 0 } else { next + 1 };
                *byte = next;
            }
            true
        })
        .unwrap();
        // Bytes 178 to 255 are skipped, so the first character is ALPHABET[0] from byte 0.
        assert_eq!(password.as_bytes()[0], ALPHABET[0]);
        assert!(is_strong(&password));
        assert!(calls >= 1);
    }

    #[test]
    fn a_failed_source_or_one_that_never_mixes_gives_none() {
        assert!(password_from(|_| false).is_none());
        // Always byte 0: 64 copies of 'A', which lacks three classes, on every draw.
        let mut calls = 0;
        assert!(
            password_from(|bytes| {
                calls += 1;
                bytes.fill(0);
                true
            })
            .is_none()
        );
        assert_eq!(calls, DRAWS);
    }

    #[test]
    fn only_a_password_generate_could_give_is_strong() {
        let good = generate().unwrap();
        assert!(is_strong(&good));
        assert!(!is_strong(""));
        assert!(!is_strong(good.get(1..).unwrap()));
        assert!(!is_strong(&format!("{}A", *good)));
        assert!(!is_strong(&"Aa1!".repeat(16).replace('!', " ")));
        assert!(is_strong(&"Aa1!".repeat(16)));
        assert!(!is_strong(&"Aa11".repeat(16)));
        assert!(!is_strong(&"aa1!".repeat(16)));
        assert!(!is_strong(&"AA1!".repeat(16)));
        assert!(!is_strong(&"Aaa!".repeat(16)));
        // 64 bytes, but not 64 characters of the alphabet.
        assert!(!is_strong(&format!(
            "{}é",
            "Aa1!".repeat(16).get(..62).unwrap()
        )));
    }
}
