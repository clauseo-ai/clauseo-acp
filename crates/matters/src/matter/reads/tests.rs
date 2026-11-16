//! The Windows rule for a path's parts, as every OS can check it.

use super::windows_invalid;

#[test]
fn parts_windows_reads_otherwise_are_invalid() {
    for part in [
        "C:x",
        "notes.txt:hidden",
        "CON",
        "nul.txt",
        "Com1.docx",
        "LPT¹",
        "trailing.",
        "trailing ",
        "CON .txt",
    ] {
        assert!(windows_invalid(part), "{part:?}");
    }
    for part in [
        "brief.txt",
        "CONSOLE.txt",
        "com10",
        ".gitignore",
        "a.b.c",
        "names with spaces",
    ] {
        assert!(!windows_invalid(part), "{part:?}");
    }
}
