use super::{bounds, holds, raw_text};
use crate::pre_write_guard::words::Escape;

#[test]
fn a_name_counts_only_as_a_whole_name() {
    for text in [
        "cat Contract.docx",
        "cat ./Contract.docx",
        r"type C:\m\Contract.docx",
        "x>Contract.docx",
        "cp a 'Contract.docx'",
        "--out=Contract.docx",
        "-Path:Contract.docx",
        "cat Contract.docx;ls",
        "Contract.docx",
        "(Contract.docx)",
    ] {
        assert!(holds(text, "Contract.docx"), "{text}");
    }
    for text in [
        "cat Contract.docx.bak",
        "cat MyContract.docx",
        "cat Contract.docx/x",
        "cat Contract",
        "cat Contract.docxx",
    ] {
        assert!(!holds(text, "Contract.docx"), "{text}");
    }
    assert!(!holds("anything", ""));
    assert!(bounds('/', true) && !bounds('/', false));
}

#[test]
fn the_raw_text_joins_lines_and_decodes_ansi_c_quotes() {
    assert_eq!(
        raw_text(
            "echo > Con\\\ntract.docx $'Con\\x74ract.docx'",
            Escape::Backslash
        ),
        "echo > Contract.docx Contract.docx"
    );
    assert_eq!(
        raw_text("Set-Content Con`\ntract.docx $'a'", Escape::Backtick),
        "Set-Content Contract.docx $'a'"
    );
}
