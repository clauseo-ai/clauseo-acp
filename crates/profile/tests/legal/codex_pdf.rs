//! The lines Codex gets about scanned and long PDFs.

/// Codex has no PDF input, so where Clauseo's `pdftoppm` is on its `PATH` the desktop tells it
/// to render pages into its work folder and view them, and that `pdftotext` prints a PDF's text
/// with OCR for scans (`setup/hook.rs`). The profile's own instructions never say so, for either
/// agent: the tools are not everywhere, so these lines are not in `budgets.rs`'s `CODEX_MAX`.
#[test]
fn the_pdf_lines_are_two_short_sentences_the_profile_never_adds_itself() {
    use clauseo_profile::instructions::{codex_pdf, word_count};
    use clauseo_protocol::AgentKind;
    let line = codex_pdf();
    let sentences: Vec<&str> = line.split(". ").collect();
    assert_eq!(sentences.len(), 2, "two sentences: {line}");
    assert!(
        word_count(sentences[0]) <= 40,
        "{} words",
        word_count(sentences[0])
    );
    assert!(
        word_count(sentences[1]) <= 30,
        "{} words",
        word_count(sentences[1])
    );
    assert!(
        sentences[1].starts_with("`pdftotext \"<file>\" -` prints a PDF's text")
            && sentences[1].contains("by OCR"),
        "{line}"
    );
    assert!(
        line.contains("`pdftoppm -png -r 100 -f <first> -l <last> "),
        "{line}"
    );
    assert!(line.contains("\"{work_folder}/<name>\""), "{line}");
    let profile = super::launch_profile(&std::env::temp_dir().join("codex-pdf-line"));
    for macos in [true, false] {
        for tool in ["pdftoppm", "pdftotext"] {
            assert!(!profile.codex_instructions_for(macos).contains(tool));
            assert!(!profile.claude_instructions_for(macos).contains(tool));
        }
    }
    let codex = profile.codex_keys_without_index(Some(AgentKind::Codex));
    let text = codex["developer_instructions"].as_str().unwrap();
    assert!(!text.contains("pdftoppm"));
    assert!(!profile.instructions().contains("pdftoppm"));
}
