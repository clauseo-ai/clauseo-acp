//! The lines Codex gets about scanned and long PDFs.

/// Codex has no PDF input, so where Clauseo's `pdftoppm` is on its `PATH` the desktop tells it
/// to render pages into its work folder and view them, and that `pdftotext` prints a PDF's text
/// with OCR for scans, JPEG, PNG and TIFF ones too (`setup/hook.rs`). The profile's own Codex
/// instructions never say so: the tools are not everywhere, so these lines are not in
/// `budgets.rs`'s `CODEX_MAX`. Claude's "Working at scale" names `pdftotext` for TIFF scans,
/// which its Read cannot show (counted in `CLAUDE_MAX`).
#[test]
fn the_pdf_lines_are_three_short_sentences_the_profile_never_adds_itself() {
    use clauseo_profile::instructions::{codex_pdf, word_count};
    use clauseo_protocol::AgentKind;
    let line = codex_pdf();
    let sentences: Vec<&str> = line.split(". ").collect();
    assert_eq!(sentences.len(), 3, "three sentences: {line}");
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
    assert_eq!(
        sentences[2].trim_end(),
        "`pdftotext` reads JPEG, PNG and TIFF scans the same way."
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
        }
        // Claude's own clause ("Working at scale", scale-claude.txt) names pdftotext, for TIFF
        // scans its Read cannot show; never pdftoppm, which its Read runs itself.
        let claude = profile.claude_instructions_for(macos);
        assert!(!claude.contains("pdftoppm"));
        assert!(
            claude.contains(
                "For a .tif/.tiff scan, or a scanned PDF you cannot make out, run \
                 `pdftotext -layout \"<file>\" -`."
            ),
            "{claude}"
        );
    }
    let codex = profile.codex_keys_without_index(Some(AgentKind::Codex));
    let text = codex["developer_instructions"].as_str().unwrap();
    assert!(!text.contains("pdftoppm"));
    assert!(!profile.instructions().contains("pdftoppm"));
}
