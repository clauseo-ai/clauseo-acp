//! How many tracked changes a view shows, against how many stetdoc counted, for `parse.rs`.
//!
//! It answers:
//! - how many stetdoc counted: the head's `{summary: 6 tracked changes (C 6) · …}`, which counts
//!   every story's changes (the body, headers and footers, notes, comments);
//! - which a line shows, by one rule for every tag: a `#<id> <author>` pair in a marker on it
//!   names a change (`{#3 C}`, `{rev run #0 C}`, `{¶- #2 RT}`, `{row inserted #16 C}`,
//!   `{#15 JD move from #13}`, `#1,2` for several), a marker found as the parser finds one (an
//!   escaped brace is text, a brace inside a quoted string is the string's), whether or not the
//!   rest of the line reads. An id after `from`, `to` or `peer` is another change's, named by
//!   its own marker; an id with no author after it is counted and noted, so the blackline says
//!   it may differ.
//!
//! The body's lines are scanned up to `{appendix}`, after which stetdoc lists the notes the page
//! cites in their final text, with no marks: a change there is one the blackline does not show.

use std::collections::BTreeSet;

/// What a line says as the head's summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Summary {
    /// Another line, not the summary.
    Other,
    /// The summary, counting this many tracked changes (0 when it names none).
    Count(usize),
    /// A summary that speaks of tracked changes in words this reader can't read: the count
    /// check can't be made.
    Unreadable,
}

/// What `line` says as the head's summary (`{summary: 6 tracked changes (C 6) · …}`, a
/// thousands separator allowed: `1,234 tracked changes`).
pub(super) fn summary_count(line: &str) -> Summary {
    let Some(rest) = line.strip_prefix("{summary:") else {
        return Summary::Other;
    };
    let first = rest.trim_start().split(" · ").next().unwrap_or_default();
    if let Some(number) = first.find(" tracked change").and_then(|at| first.get(..at)) {
        let digits: String = number.chars().filter(|c| *c != ',').collect();
        let grouped = !number.starts_with(',') && !number.ends_with(',') && !number.contains(",,");
        return match digits.parse() {
            Ok(count) if grouped && digits.bytes().all(|b| b.is_ascii_digit()) => {
                Summary::Count(count)
            }
            _ => Summary::Unreadable,
        };
    }
    if rest.contains("tracked") || rest.contains("change") {
        Summary::Unreadable
    } else {
        Summary::Count(0)
    }
}

/// Adds the change ids `line`'s markers name to `ids`; whether one of them lacks its author.
pub(super) fn ids_in(line: &str, ids: &mut BTreeSet<u64>) -> bool {
    let bytes = line.as_bytes();
    let mut at = 0;
    let mut orphan = false;
    while let Some(&b) = bytes.get(at) {
        match b {
            b'\\' => at += 2,
            b'{' if !bytes
                .get(at..)
                .is_some_and(|r| r.starts_with(b"{++") || r.starts_with(b"{--")) =>
            {
                at = marker_ids(bytes, at + 1, ids, &mut orphan);
            }
            _ => at += 1,
        }
    }
    orphan
}

/// Reads the marker whose body starts at `start`, adding the ids its `#<id> <author>` words
/// name to `ids` (and noting in `orphan` an id with no author after it); where the marker ends
/// (after its `}`, or the line's end).
fn marker_ids(bytes: &[u8], start: usize, ids: &mut BTreeSet<u64>, orphan: &mut bool) -> usize {
    let mut at = start;
    let mut quoted = false;
    let mut word_start = start;
    let mut words: Vec<&[u8]> = Vec::new();
    let end = loop {
        let b = bytes.get(at).copied();
        let ends_word = matches!(b, None | Some(b' ' | b'}')) && !quoted;
        if ends_word {
            words.push(bytes.get(word_start..at).unwrap_or_default());
            word_start = at + 1;
        }
        match b {
            None => break at,
            Some(b'\\') => at += 2,
            Some(b'"') => {
                quoted = !quoted;
                at += 1;
            }
            Some(b'}') if !quoted => break at + 1,
            Some(_) => at += 1,
        }
    };
    for (index, word) in words.iter().enumerate() {
        let Some(found) = word_ids(word) else {
            continue;
        };
        // `move from #13` names another change, which its own marker names with its author.
        let before = index.checked_sub(1).and_then(|i| words.get(i)).copied();
        if matches!(before, Some(b"from" | b"to" | b"peer")) {
            continue;
        }
        let author = words.get(index + 1).is_some_and(|next| is_author(next));
        *orphan |= !author;
        ids.extend(found);
    }
    end
}

/// Whether `word` is an author's code as the view prints it (`C`, `JD`, `R2`): a capital
/// letter, then letters and digits.
fn is_author(word: &[u8]) -> bool {
    word.first().is_some_and(u8::is_ascii_uppercase) && word.iter().all(u8::is_ascii_alphanumeric)
}

/// The ids of one word of a marker, `#3` or `#1,2`; `None` for any other word.
fn word_ids(word: &[u8]) -> Option<Vec<u64>> {
    let list = std::str::from_utf8(word.strip_prefix(b"#")?).ok()?;
    list.split(',').map(|id| id.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::Summary::{Count, Other, Unreadable};
    use super::{ids_in, summary_count};

    fn ids(line: &str) -> Vec<u64> {
        let mut found = BTreeSet::new();
        ids_in(line, &mut found);
        found.into_iter().collect()
    }

    fn orphan(line: &str) -> bool {
        ids_in(line, &mut BTreeSet::new())
    }

    #[test]
    fn ids_are_read_from_markers_only() {
        assert_eq!(ids("p1 a {--b--}{#3 C}{++c++}{#2 C} d"), [2, 3]);
        assert_eq!(ids("p2 {rev run #0 C} {¶- #7 RT} x"), [0, 7]);
        assert_eq!(ids("r1 {row inserted #16 C}"), [16]);
        assert_eq!(
            ids("p3 {++x++}{#15 JD move from #13} {#1,4 C}"),
            [1, 4, 13, 15]
        );
        // Text, escaped braces and quoted strings name no change.
        assert_eq!(
            ids(r#"p4 #5 \{#6 C\} {link "{#7 C}"}x{/link}"#),
            Vec::<u64>::new()
        );
        assert_eq!(ids("p5 {#9 C"), [9]);
    }

    /// An id is a change's when its author follows it; another change named after `from`, `to`
    /// or `peer` is that change's own marker's to name. An id with no author is still counted,
    /// and noted.
    #[test]
    fn an_id_carries_its_author() {
        assert_eq!(ids("p1 {++x++}{#15 JD move from #13}"), [15]);
        assert_eq!(
            ids("p1 {section 1 ends: next-page mark inserted #3 C peer #4}"),
            [3]
        );
        assert!(!orphan(
            "p1 {--x--}{#31 R move to -} {rev numbering #2 CL2}"
        ));
        for line in ["p1 {++x++}{#7}", "p1 {rev run #7 move}", "p1 {img #7 c}"] {
            assert!(orphan(line), "{line}");
            assert_eq!(ids(line), [7], "{line}");
        }
        assert!(!orphan("p1 {field REF \"#5\" label} {row item=2}"));
    }

    #[test]
    fn the_summary_gives_stetdocs_count() {
        let count = summary_count;
        let six = "{summary: 6 tracked changes (C 6) · 1 footnote · 1 section}";
        assert_eq!(count(six), Count(6));
        assert_eq!(
            count("{summary: 1 tracked change (C 1) · 1 section}"),
            Count(1)
        );
        assert_eq!(
            count("{summary: 1,234 tracked changes (C 1,234)}"),
            Count(1234)
        );
        assert_eq!(count("{summary: 1 section}"), Count(0));
        assert_eq!(
            count("{summary: 1 section · 2 read-only paragraphs}"),
            Count(0)
        );
        assert_eq!(count("{authors: C \"Clauseo\"}"), Other);
        // Wording or a number this reader can't read turns the check off, and says so.
        for odd in [
            "{summary: many tracked changes (C 6)}",
            "{summary: 1.234 tracked changes}",
            "{summary: 6 tracked revisions}",
            "{summary: 3 changes}",
            "{summary: ,12 tracked changes}",
        ] {
            assert_eq!(count(odd), Unreadable, "{odd}");
        }
    }
}
