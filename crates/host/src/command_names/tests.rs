use std::collections::BTreeSet;

use super::{add_json, add_words};

fn names_of(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    add_json(&mut names, text);
    names
}

#[test]
fn a_pandoc_command_names_its_output_quoted_or_not_with_or_without_a_path() {
    let names = names_of(r#"{"command":"pandoc in.md -o \"Reply to notice.docx\""}"#);
    assert!(names.contains("Reply to notice.docx"), "{names:?}");
    assert!(names.contains("in.md"));
    let names = names_of(r#"["bash","-lc","pandoc a.md -o out/Reply.docx && cp x 'b c.txt'"]"#);
    assert!(names.contains("Reply.docx"), "{names:?}");
    assert!(names.contains("b c.txt"));
    let mut names = BTreeSet::new();
    add_words(
        &mut names,
        r"pandoc a.md --output=C:\Matter\Reply.docx > log.txt",
    );
    assert!(names.contains("Reply.docx"), "{names:?}");
    assert!(names.contains("log.txt"));
    add_words(&mut names, "Wrote memo.txt.");
    assert!(names.contains("memo.txt"));
}

#[test]
fn a_name_matches_only_whole() {
    let names = names_of(r#"{"command":"cp Reply.docx.bak backup/"}"#);
    assert!(!names.contains("Reply.docx"), "{names:?}");
    let names = names_of(r#"{"command":"pandoc a.md -o Reply.docx"}"#);
    assert!(!names.contains("Reply.docx.bak"));
    assert!(!names.contains("Reply"));
}
