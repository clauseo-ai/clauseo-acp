//! The case-law hint's heuristic (R24, PR6; Q8: once per matter, dismissible). When a turn ends,
//! the host looks at its `WebSearchRecorded { source: Web }` records; if any query or result URL
//! looks like Indian case law, and the rest of the checks pass (`actor/connectors/hint.rs`), it
//! offers the connector with `HostEvent::CaseLawHint`.
//!
//! What fires, on each query and each result URL (hand-written: the host has no regex crate):
//! - citations, case-sensitive, as whole words: `SCC`, `AIR`, `SCR`, `INSC`, and a neutral
//!   citation `YYYY:COURT:N` such as `2023:DHC:1234` (`(19|20)\d{2}:[A-Z]{2,8}:\d+`; the other
//!   neutral form, `2024 INSC 12`, holds `INSC`);
//! - case names, case-insensitive: ` v. `, ` vs `, ` vs. `;
//! - courts, case-insensitive: "supreme court", "high court", "nclat", "tribunal";
//! - words, case-insensitive: "judgment", "judgement", "precedent";
//! - a result whose host is on S27's seed list or a subdomain of one, or `indiankanoon.org`
//!   (with or without `www.`, and any other subdomain).
//!
//! Deterministic, from what S26 already records; nothing leaves the machine to decide it.
//!
//! `hints.json` (`hint/file.rs`) remembers the matters where it was shown or dismissed.

pub(crate) mod file;

use clauseo_protocol::{SearchSource, WebSearchRecorded};

use crate::browser::seed_sites::SEED_SITES;

/// The citation marks, matched case-sensitively as whole words.
const CITATIONS: [&str; 4] = ["SCC", "AIR", "SCR", "INSC"];
/// The phrases matched case-insensitively anywhere: courts and words.
const PHRASES: [&str; 7] = [
    "supreme court",
    "high court",
    "nclat",
    "tribunal",
    "judgment",
    "judgement",
    "precedent",
];
/// The case-name marks, matched case-insensitively with the spaces shown (the text's ends count
/// as spaces).
const CASE_NAMES: [&str; 3] = [" v. ", " vs ", " vs. "];
/// The case-law site the connector reads, beside the seed list.
const KANOON: &str = "indiankanoon.org";

/// What a turn's web searches looked like, for its end.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnLooks {
    /// Nothing that fires.
    #[default]
    Nothing,
    /// A search that looked like case law.
    CaseLaw,
}

/// Whether a web search record looks like a search for Indian case law. A case-law record of
/// the connector's own never counts: the hint offers the connector to those without it.
pub(crate) fn fires(record: &WebSearchRecorded) -> bool {
    record.source == SearchSource::Web
        && (text_fires(&record.query)
            || record
                .results
                .iter()
                .any(|result| text_fires(&result.url) || host_fires(&result.url)))
}

/// Whether a query (or a URL, read as text) holds a citation, a case name, a court or a word.
pub(crate) fn text_fires(text: &str) -> bool {
    if CITATIONS.iter().any(|mark| has_word(text, mark)) || has_neutral_citation(text) {
        return true;
    }
    let lower = text.to_lowercase();
    if PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return true;
    }
    let padded = format!(" {lower} ");
    CASE_NAMES.iter().any(|mark| padded.contains(mark))
}

/// Whether a result URL's host is a case-law site: on the seed list or under one, or Indian
/// Kanoon.
pub(crate) fn host_fires(url: &str) -> bool {
    let Some(host) = host_of(url) else {
        return false;
    };
    SEED_SITES
        .iter()
        .chain(std::iter::once(&KANOON))
        .any(|site| {
            host == *site
                || host
                    .strip_suffix(site)
                    .is_some_and(|rest| rest.ends_with('.'))
        })
}

/// A URL's host, lower case, without user info, port or a trailing dot.
fn host_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = host_port.split(':').next()?;
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// A word character, as a regex's `\w`: a letter, a digit or `_`.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether `word` appears in `text` with no word character just before or after it.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        let after = text
            .get(at + word.len()..)
            .and_then(|tail| tail.chars().next());
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

/// Whether `text` holds a neutral citation `YYYY:COURT:N` as a whole word: a year from 1900 to
/// 2099, 2 to 8 capital letters, and digits.
fn has_neutral_citation(text: &str) -> bool {
    let bytes = text.as_bytes();
    (0..bytes.len()).any(|start| {
        let before = start.checked_sub(1).and_then(|at| bytes.get(at)).copied();
        !before.is_some_and(is_word_byte) && bytes.get(start..).is_some_and(neutral_at)
    })
}

/// Whether a neutral citation starts at the head of `bytes` and ends at a word boundary.
fn neutral_at(bytes: &[u8]) -> bool {
    let digits = |from: &[u8]| from.iter().take_while(|b| b.is_ascii_digit()).count();
    let year = bytes.get(..4).is_some_and(|year| {
        digits(year) == 4 && (year.starts_with(b"19") || year.starts_with(b"20"))
    });
    let Some(rest) = bytes
        .get(4..)
        .filter(|_| year)
        .and_then(|rest| rest.strip_prefix(b":"))
    else {
        return false;
    };
    let court = rest.iter().take_while(|b| b.is_ascii_uppercase()).count();
    let Some(number) = rest
        .get(court..)
        .filter(|_| (2..=8).contains(&court))
        .and_then(|after| after.strip_prefix(b":"))
    else {
        return false;
    };
    let count = digits(number);
    count > 0 && !number.get(count).copied().is_some_and(is_word_byte)
}

/// A word byte: an ASCII letter, digit or `_`, or any byte of a non-ASCII character (which
/// `\w` would count as a letter in most scripts; conservative for a boundary).
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || !byte.is_ascii()
}

#[cfg(test)]
mod tests;
