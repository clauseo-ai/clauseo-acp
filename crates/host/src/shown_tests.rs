//! The host's one rule for approved text: every class it writes out, the isolates, and that what
//! it shows reads back one way only (a property test over argument lists).

use icu_properties::CodePointMapData;
use icu_properties::props::GeneralCategory;
use proptest::prelude::*;

use super::{argv as program, text};

/// Every class the confirm writes out, one character each: none is drawn as itself.
#[test]
fn line_breaks_blanks_and_invisible_characters_are_written_out() {
    let cases = [
        ('\u{2028}', "line separator (Zl)"),
        ('\u{2029}', "paragraph separator (Zp)"),
        ('\u{0085}', "next line (Cc)"),
        ('\u{00a0}', "no-break space (Zs)"),
        ('\u{3000}', "ideographic space (Zs)"),
        ('\u{00ad}', "soft hyphen (Cf)"),
        ('\u{061c}', "Arabic letter mark (Cf)"),
        ('\u{180e}', "Mongolian vowel separator (Cf)"),
        ('\u{206a}', "inhibit symmetric swapping (Cf)"),
        ('\u{fff9}', "interlinear annotation (Cf)"),
        ('\u{e0041}', "tag (Cf)"),
        ('\u{fe0f}', "variation selector (ignorable)"),
        ('\u{e0100}', "variation selector supplement (ignorable)"),
        ('\u{3164}', "Hangul filler (ignorable)"),
        ('\u{115f}', "Hangul choseong filler (ignorable)"),
        ('\u{1160}', "Hangul jungseong filler (ignorable)"),
        ('\u{ffa0}', "halfwidth Hangul filler (ignorable)"),
        ('\u{2800}', "blank Braille pattern"),
        ('\u{2068}', "first-strong isolate (Cf)"),
    ];
    for (c, what) in cases {
        let shown = program(&[format!("a{c}b")]);
        assert!(!shown.contains(c), "{what} drawn as itself: {shown:?}");
        assert_eq!(shown, format!("a{}b", c.escape_default()), "{what}");
    }
    // A plain space still parts arguments, and letters and marks stay as they are.
    assert_eq!(program(&["a b".to_owned()]), "'a b'");
    assert_eq!(program(&["é𝔄".to_owned()]), "é𝔄");
}

/// The tables are Unicode 17's: Tai Yo (U+1E6C0, new in 17.0) is a letter.
#[test]
fn the_tables_are_unicode_17() {
    let category = CodePointMapData::<GeneralCategory>::new().get('\u{1e6c0}');
    assert_eq!(category, GeneralCategory::OtherLetter);
}

/// Right-to-left text sits between isolates, so it can't carry the digits beside it; a program's
/// own isolates are written out.
#[test]
fn right_to_left_parts_are_isolated() {
    let shown = program(&["x".to_owned(), "א".to_owned(), "2".to_owned()]);
    assert_eq!(shown, "x \u{2066}\u{2067}א\u{2069}\u{2069} 2");
    assert_eq!(
        text("https://a.example/\u{5d0}"),
        "\u{2066}https://a.example/\u{2067}א\u{2069}\u{2069}"
    );
    assert_eq!(program(&["a b".to_owned()]), "'a b'");
}

/// Reads a shown program back to its parts: the inverse of [`program`].
fn read_back(shown: &str) -> Vec<String> {
    let text: Vec<char> = shown
        .chars()
        .filter(|c| !matches!(c, '\u{2066}'..='\u{2069}'))
        .collect();
    let mut parts = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let (raw, next) = match text[at] {
            ' ' => {
                at += 1;
                continue;
            }
            '\'' => {
                let end = (at + 1..text.len()).find(|&i| text[i] == '\'').unwrap();
                (text[at + 1..end].to_vec(), end + 1)
            }
            '"' => {
                let mut end = at + 1;
                while text[end] != '"' {
                    end += if text[end] == '\\' { 2 } else { 1 };
                }
                (text[at + 1..end].to_vec(), end + 1)
            }
            _ => {
                let end = (at..text.len())
                    .find(|&i| text[i] == ' ')
                    .unwrap_or(text.len());
                (text[at..end].to_vec(), end)
            }
        };
        parts.push(unescape(&raw));
        at = next;
    }
    parts
}

fn unescape(raw: &[char]) -> String {
    let mut out = String::new();
    let mut at = 0;
    while at < raw.len() {
        if raw[at] != '\\' {
            out.push(raw[at]);
            at += 1;
            continue;
        }
        match raw[at + 1] {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'u' => {
                let close = (at..raw.len()).find(|&i| raw[i] == '}').unwrap();
                let hex: String = raw[at + 3..close].iter().collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                at = close + 1;
                continue;
            }
            other => out.push(other),
        }
        at += 2;
    }
    out
}

const PIECES: &[char] = &[
    'a', 'n', 'u', '{', '}', '2', 'e', '\\', '\n', '\t', ' ', '\'', '"', '\u{202e}', '\u{2028}',
    '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}', '\u{3164}', 'א', 'ב', '٣', 'ع', 'é', '1', '.',
    '/', '\u{0301}', '\u{05b4}', '\u{e000}',
];

fn parts() -> impl Strategy<Value = Vec<String>> {
    let part = proptest::collection::vec(proptest::sample::select(PIECES), 0..8)
        .prop_map(|chars| chars.into_iter().collect::<String>());
    proptest::collection::vec(part, 0..4)
}

proptest! {
    /// What is shown reads back to exactly the parts it came from, so two different argument
    /// lists never show the same.
    #[test]
    fn a_shown_program_reads_back_to_its_parts(parts in parts()) {
        prop_assert_eq!(read_back(&program(&parts)), parts);
    }

    #[test]
    fn two_different_programs_never_show_the_same(a in parts(), b in parts()) {
        prop_assume!(a != b);
        prop_assert_ne!(program(&a), program(&b));
    }
}

#[test]
fn a_real_newline_and_a_written_one_show_apart() {
    assert_ne!(
        program(&["a\nb".to_owned()]),
        program(&["a\\nb".to_owned()])
    );
    assert_ne!(
        program(&["\u{202e}".to_owned()]),
        program(&["\\u{202e}".to_owned()])
    );
}

/// Free text keeps nothing invisible and stays on one line.
#[test]
fn free_text_is_written_out_on_one_line() {
    assert_eq!(
        text("Run it?\u{2028}\u{3164}rm"),
        "Run it?\\u{2028}\\u{3164}rm"
    );
    assert_eq!(text("a\\nb\n"), "a\\\\nb\\n");
}

/// Hebrew or Arabic beside digits, punctuation and paths: the part is forced left to right and
/// each right-to-left run is isolated on its own, so the digits stay where they run.
#[test]
fn right_to_left_runs_are_isolated_one_by_one_in_a_left_to_right_part() {
    const LRI: char = '\u{2066}';
    const RLI: char = '\u{2067}';
    const PDI: char = '\u{2069}';
    assert_eq!(text("cp א 1 2"), format!("{LRI}cp {RLI}א{PDI} 1 2{PDI}"));
    assert_eq!(
        program(&["sh".to_owned(), "-c".to_owned(), "cp א 1 2".to_owned()]),
        format!("sh -c {LRI}'cp {RLI}א{PDI} 1 2'{PDI}")
    );
    // Two words, a path between: each word its own run, the path left to right between them.
    assert_eq!(
        text("שלום /tmp/a.txt עולם"),
        format!("{LRI}{RLI}שלום{PDI} /tmp/a.txt {RLI}עולם{PDI}{PDI}")
    );
    // Arabic-Indic digits are isolated as the letters are; European digits stay outside.
    assert_eq!(
        text("ملف ٣ (2).pdf"),
        format!("{LRI}{RLI}ملف{PDI} {RLI}٣{PDI} (2).pdf{PDI}")
    );
    // A mark on a Hebrew letter stays inside its run.
    assert_eq!(
        text("א\u{05b4}1"),
        format!("{LRI}{RLI}א\u{05b4}{PDI}1{PDI}")
    );
}

/// Private-use and unassigned characters draw as escapes, never as an icon or as whatever a later
/// Unicode makes them.
#[test]
fn private_use_and_unassigned_characters_are_written_out() {
    for c in [
        '\u{f8ff}',
        '\u{e000}',
        '\u{100000}',
        '\u{10fffd}',
        '\u{0378}',
        '\u{fffe}',
    ] {
        assert_eq!(text(&format!("a{c}b")), format!("a{}b", c.escape_default()));
    }
}

/// At most three combining marks draw on one letter; the rest are written out, so a stack can't
/// cover the line above.
#[test]
fn combining_marks_past_the_third_are_written_out() {
    assert_eq!(text("e\u{301}\u{302}\u{303}"), "e\u{301}\u{302}\u{303}");
    let stacked = format!("a{}", "\u{369}".repeat(60));
    let shown = text(&stacked);
    assert!(
        shown.starts_with("a\u{369}\u{369}\u{369}\\u{369}"),
        "{shown}"
    );
    assert_eq!(shown.matches('\u{369}').count(), 3);
    // A new letter starts a new count.
    assert_eq!(text("a\u{301}b\u{301}"), "a\u{301}b\u{301}");
}
