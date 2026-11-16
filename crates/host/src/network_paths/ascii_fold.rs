//! The code points a case-insensitive volume may open as an ASCII letter, for every match that
//! refuses a name: [`name_folds_to`] and [`refused_in_other_matter`].
//!
//! Vendored from `clauseo_sandbox` (stage C, #735); removed when that lands. It is stage C's
//! CaseFolding-derived table for ASCII targets, and these two functions have the signatures
//! `clauseo_sandbox` exports, so the swap is an import change.
//!
//! APFS matches names by Unicode case folding, so `.clauſeo` (U+017F, long s) opens `.clauseo`
//! and `.agentſ` opens `.agents` (measured on this Mac's volume). Seatbelt matched the stored
//! name, so its ASCII-only rule refused both too, but the policy sees names as spelled, and the
//! profile shouldn't rest on how Seatbelt resolves. [`BEYOND_ASCII`] lists every code point
//! outside ASCII that folds to an ASCII letter in Unicode 16's CaseFolding.txt, with statuses C
//! and S, and also T (Turkic), which refuses more and is safe. A test checks the table against
//! the excerpt of that file in `ascii_fold/case_folding_ascii.txt`, and against every code
//! point's full fold by `caseless`.
//!
//! Normalisation: APFS also opens an NFC and an NFD spelling as one name. The refused names are
//! ASCII, and the one code point whose canonical decomposition is an ASCII letter, U+212A KELVIN
//! SIGN to `K`, folds to `k` and is in the table. So case is all that matters here. Full folding
//! (status F) maps nothing to a single ASCII letter, and its ASCII-only folds (`ß` to `ss`, `ﬆ`
//! to `st`, `ﬁ` to `fi` and the like) spell no pair of letters in a refused name; a test checks
//! that too.

use std::ffi::OsStr;

/// Each code point outside ASCII that folds to an ASCII letter, with the letter. Kept in step
/// with `case_folding_ascii.txt` by a test.
pub(crate) const BEYOND_ASCII: [(char, char); 4] = [
    // 0130; T; 0069: LATIN CAPITAL LETTER I WITH DOT ABOVE.
    ('\u{130}', 'i'),
    // 0049; T; 0131: LATIN SMALL LETTER DOTLESS I, Turkic `I`.
    ('\u{131}', 'i'),
    // 017F; C; 0073: LATIN SMALL LETTER LONG S.
    ('\u{17F}', 's'),
    // 212A; C; 006B: KELVIN SIGN.
    ('\u{212A}', 'k'),
];

/// The lower-case ASCII letter `c` folds to, if it folds to one.
pub(crate) fn ascii_letter(c: char) -> Option<char> {
    if c.is_ascii_alphabetic() {
        return Some(c.to_ascii_lowercase());
    }
    BEYOND_ASCII
        .iter()
        .find(|(from, _)| *from == c)
        .map(|(_, to)| *to)
}

/// Every code point that folds to the ASCII letter `letter`: its lower and upper case, then the
/// ones in [`BEYOND_ASCII`].
#[cfg(test)]
pub(crate) fn spellings(letter: char) -> Vec<char> {
    let letter = letter.to_ascii_lowercase();
    let mut out = vec![letter, letter.to_ascii_uppercase()];
    out.extend(
        BEYOND_ASCII
            .iter()
            .filter(|(_, to)| *to == letter)
            .map(|(from, _)| *from),
    );
    out
}

/// Whether `name` is `ascii` once each letter is folded: every code point of `name` is either
/// one of [`spellings`] of the letter at its place in `ascii`, or, where `ascii` has no letter,
/// that same character. A name that is not Unicode compares without ASCII case.
fn folds_to(name: &OsStr, ascii: &str) -> bool {
    let Some(text) = name.to_str() else {
        return name.eq_ignore_ascii_case(ascii);
    };
    let mut want = ascii.chars();
    text.chars().all(|c| {
        want.next().is_some_and(|w| match ascii_letter(c) {
            Some(letter) => letter == w.to_ascii_lowercase(),
            None => c == w,
        })
    }) && want.next().is_none()
}

/// The names refused in a read of another matter, and refused by name wherever a deny matches
/// one: Clauseo's own folder, Git's, Claude's, Codex's and the agents' skills folder.
pub(crate) const REFUSED_IN_OTHER_MATTER: [&str; 5] =
    [".git", ".clauseo", ".claude", ".codex", ".agents"];

/// Whether `name` is the ASCII name `ascii` as a case-insensitive volume opens it: ASCII case,
/// and each code point of [`BEYOND_ASCII`] (`ſ`, the Kelvin sign, `ı`, `İ`) as its letter.
/// For a match that refuses; never for one that allows.
pub(crate) fn name_folds_to(name: &OsStr, ascii: &str) -> bool {
    folds_to(name, ascii)
}

/// Whether `name` is one of [`REFUSED_IN_OTHER_MATTER`] ([`name_folds_to`]).
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "stage E's #764 takes it from here; the host refuses .codex alone"
    )
)]
pub(crate) fn refused_in_other_matter(name: &OsStr) -> bool {
    REFUSED_IN_OTHER_MATTER
        .iter()
        .any(|refused| name_folds_to(name, refused))
}

#[cfg(test)]
mod tests;
