//! `validate()`'s rules for prompt shape, matter names and titles, and the public limits.

use clauseo_protocol::{
    HostCommand, MAX_MATTER_NAME_BYTES, MAX_PROMPT_BLOCKS, MAX_PROMPT_FILES, MatterSource,
    PromptBlock,
};

use super::fixtures::{folder_ref_id, thread_id};
use super::{edit_queued, files, invalid, send_prompt, text, too_large};

#[test]
fn prompt_shape() {
    let builders: [fn(Vec<PromptBlock>) -> HostCommand; 2] = [send_prompt, edit_queued];
    for build in builders {
        // Blocks: 100 pass, 101 fail (files stay at 50 or fewer).
        let mut hundred = files(MAX_PROMPT_FILES);
        hundred.extend((0..50).map(|_| text("x")));
        assert!(build(hundred.clone()).validate().is_ok());
        hundred.push(text("x"));
        assert!(too_large(&build(hundred).validate(), 101, 100));
        // No empty text block, even next to a real one.
        assert!(build(vec![text("a")]).validate().is_ok());
        assert!(invalid(&build(vec![text("a"), text("")]).validate()));
        // Something to send: one visible character, or one file.
        assert!(build(vec![text(" \n"), text(" a ")]).validate().is_ok());
        assert!(
            build(vec![text(" "), files(1).remove(0)])
                .validate()
                .is_ok()
        );
        assert!(invalid(&build(vec![text(" \n\t"), text(" ")]).validate()));
    }
}

#[test]
fn matter_name_rules() {
    let create = |name: &str| HostCommand::CreateMatter {
        name: name.to_owned(),
        source: MatterSource::ExistingFolder {
            folder: folder_ref_id(3),
        },
    };
    let ok = |name: &str| assert!(create(name).validate().is_ok(), "{name:?} refused");
    let bad = |name: &str| assert!(invalid(&create(name).validate()), "{name:?} accepted");
    // Bytes: 255 pass, 256 fail (3-byte characters, within 200 characters).
    ok(&"€".repeat(85));
    let long = create(&("€".repeat(85) + "a")).validate();
    assert!(too_large(&long, 256, 255), "{long:?}");
    // Characters: 200 pass, 201 fail.
    ok(&"a".repeat(200));
    bad(&"a".repeat(201));
    for c in ['<', '>', ':', '"', '/', '\\', '|', '?', '*', '\u{7}'] {
        bad(&format!("a{c}b"));
    }
    ok("a.b c-d_(1)");
    bad("trailing.");
    bad("trailing ");
    bad(" leading");
    ok("inner. space");
    bad(".");
    bad("..");
    ok("...a");
    for reserved in ["CON", "con", "Nul.txt", "com1", "LPT9.doc", "aux.tar.gz"] {
        bad(reserved);
    }
    for near in ["CONSOLE", "com10", "lpt00", "my con", "nul_x"] {
        ok(near);
    }
}

#[test]
fn title_visibility() {
    let rename = |title: &str| HostCommand::RenameThread {
        thread: thread_id(1),
        title: title.into(),
    };
    let ok = |title: &str| assert!(rename(title).validate().is_ok(), "{title:?} refused");
    let bad = |title: &str| assert!(invalid(&rename(title).validate()), "{title:?} accepted");
    ok("a");
    ok("\u{200B}a\u{FEFF}");
    for invisible in [
        "",
        " ",
        "\u{200B}",
        "\u{200C}\u{200D}",
        " \u{2060}\u{FEFF} ",
    ] {
        bad(invisible);
    }
    for bidi in ['\u{202A}', '\u{202E}', '\u{2066}', '\u{2069}'] {
        bad(&format!("a{bidi}b"));
    }
    ok("a\u{2065}b");
    ok("a\u{202F}b");
    bad("a\u{0}b");
    bad("a\u{7F}b");
    // The same rules on a matter name.
    let create = |name: &str| HostCommand::CreateMatter {
        name: name.to_owned(),
        source: MatterSource::ExistingFolder {
            folder: folder_ref_id(3),
        },
    };
    assert!(invalid(&create("a\u{202E}b").validate()));
    assert!(invalid(&create("\u{200B}").validate()));
}

#[test]
fn limits_are_public() {
    assert_eq!(MAX_PROMPT_BLOCKS, 100);
    assert_eq!(MAX_MATTER_NAME_BYTES, 255);
}

#[test]
fn separators_marks_and_fillers() {
    let rename = |title: &str| HostCommand::RenameThread {
        thread: thread_id(1),
        title: title.into(),
    };
    let ok = |title: &str| assert!(rename(title).validate().is_ok(), "{title:?} refused");
    let bad = |title: &str| assert!(invalid(&rename(title).validate()), "{title:?} accepted");
    // Line and paragraph separators are refused; a plain space is not.
    ok("a b");
    bad("a\u{2028}b");
    bad("a\u{2029}b");
    // Bidi marks are allowed inside text, embeddings and isolates are not.
    for mark in ['\u{200E}', '\u{200F}', '\u{061C}'] {
        ok(&format!("\u{5e9}{mark}1"));
    }
    bad("a\u{202B}b");
    bad("a\u{2067}b");
    // Marks, the soft hyphen and Hangul fillers show nothing.
    for invisible in [
        '\u{200E}', '\u{200F}', '\u{061C}', '\u{00AD}', '\u{3164}', '\u{115F}', '\u{1160}',
        '\u{FFA0}',
    ] {
        bad(&format!(" {invisible}"));
        ok(&format!("{invisible}x"));
    }
    // Prompts use the same visibility test.
    let builders: [fn(Vec<PromptBlock>) -> HostCommand; 2] = [send_prompt, edit_queued];
    for build in builders {
        assert!(invalid(&build(vec![text("\u{200B}")]).validate()));
        assert!(invalid(&build(vec![text("\u{3164}\u{00AD}")]).validate()));
        assert!(
            build(vec![text("\u{200B}"), files(1).remove(0)])
                .validate()
                .is_ok()
        );
        assert!(build(vec![text("\u{200B}x")]).validate().is_ok());
    }
}

#[test]
fn more_reserved_names() {
    let create = |name: &str| HostCommand::CreateMatter {
        name: name.to_owned(),
        source: MatterSource::ExistingFolder {
            folder: folder_ref_id(3),
        },
    };
    let names = [
        "COM0",
        "lpt0.txt",
        "CONIN$",
        "conout$.log",
        "COM¹",
        "com².doc",
        "COM³",
        "LPT¹",
        "lpt²",
        "LPT³.x",
    ];
    for name in names {
        assert!(invalid(&create(name).validate()), "{name:?} accepted");
    }
    for name in ["COM⁴", "CONIN", "CONOUT$X", "LPT¹0", "com0x"] {
        assert!(create(name).validate().is_ok(), "{name:?} refused");
    }
}
