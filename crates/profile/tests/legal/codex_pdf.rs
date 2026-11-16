//! The lines Codex gets about scanned and long PDFs.

/// Codex has no PDF input, so where Clauseo's `pdftoppm` is on its `PATH` the desktop tells it
/// to render pages into its work folder and view them, and that `pdftotext` prints a PDF's text
/// with OCR for scans, and to read every PDF only with them and `pdfinfo` (`setup/hook.rs`). The profile's own instructions never say so, for either
/// agent: the tools are not everywhere. `budgets.rs`'s `CODEX_MAX` counts these lines all the same,
/// as every line that can ship.
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
    assert!(
        word_count(sentences[2]) <= 30,
        "{} words",
        word_count(sentences[2])
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

/// Stage C's approval census: Codex read a PDF with `mdls` and `swift`, which its sandbox blocks,
/// and each raised a "run outside the sandbox" card that can never be always-approved. So the
/// last sentence sends every PDF read (text, a scanned page, the page count, a table, a long file)
/// to Clauseo's tools, on macOS and Windows alike, and names each command it must never use.
#[test]
fn the_pdf_line_sends_every_pdf_to_clauseos_tools_and_names_what_never_to_use() {
    use clauseo_profile::instructions::codex_pdf;
    let line = codex_pdf();
    let last = line.split(". ").nth(2).expect(line);
    assert!(
        last.starts_with(
            "Read every PDF only with these two tools and `pdfinfo \"<file>\"` for pages; "
        ),
        "{line}"
    );
    for tool in [
        "`pdftoppm -png",
        "`pdftotext \"<file>\" -`",
        "at most 20 at a time",
    ] {
        assert!(line.contains(tool), "{tool}: {line}");
    }
    let never = last.split("never use ").nth(1).expect(line);
    assert!(never.ends_with("on a PDF."), "{line}");
    for blocked in [
        "mdls",
        "swift",
        "sips",
        "qlmanage",
        "textutil",
        "osascript",
        "PowerShell COM",
    ] {
        assert!(never.contains(blocked), "{blocked}: {line}");
    }
}
