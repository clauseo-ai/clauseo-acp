//! Names and the typed fields of commands: titles ([`Title`], [`check_display_title`]),
//! agent-chosen ids ([`AgentChoiceId`]), a new matter folder's name and Windows device names.

use clauseo_protocol::{
    AgentChoiceId, CommandEnvelope, ConfigValue, Error, HostCommand, MAX_ID_TEXT_BYTES,
    MAX_TITLE_CHARS, MatterSource, PromptBlock, Title, check_display_title,
    is_windows_reserved_name,
};

use super::fixtures::{choice, command_id, err_contains, folder_ref_id, thread_id, title};
use super::{Validate, edit_queued, files, invalid, send_prompt, text, too_large};

/// Whether `result` is one of the errors a refused title or id gives.
fn refused<T>(result: &Result<T, Error>) -> bool {
    matches!(result, Err(Error::Format { .. } | Error::TooLarge { .. }))
}

fn ok_title(text: &str) {
    assert!(Title::new(text).is_ok(), "{text:?} refused");
}

/// The rule a title with no visible character breaks.
const NO_VISIBLE: &str = "a title has a visible character";

/// The rule a title with a control, bidi control or line separator character breaks.
const NO_CONTROL: &str = "a title holds no control, bidi control or line separator character";

/// Refused by `Title::new` and by `check_display_title` with `rule` in the message, and a
/// `RenameThread` holding it does not decode.
fn bad_title(text: &str, rule: &str) {
    err_contains(Title::new(text), rule);
    err_contains(check_display_title(text), rule);
    let command = HostCommand::RenameThread {
        thread: thread_id(1),
        title: title("x"),
    };
    let json = serde_json::to_string(&command).unwrap().replace(
        r#""title":"x""#,
        &format!(r#""title":{}"#, serde_json::to_string(text).unwrap()),
    );
    err_contains(serde_json::from_str::<HostCommand>(&json), rule);
}

fn create(name: &str, source: MatterSource) -> HostCommand {
    HostCommand::CreateMatter {
        name: title(name),
        source,
    }
}

fn new_folder(name: &str) -> HostCommand {
    let folder = folder_ref_id(3);
    create(name, MatterSource::NewFolder { folder })
}

fn existing_folder(name: &str) -> HostCommand {
    let folder = folder_ref_id(3);
    create(name, MatterSource::ExistingFolder { folder })
}

#[test]
fn title_length_counts_characters() {
    ok_title(&"ü".repeat(MAX_TITLE_CHARS));
    let long = Title::new(&"ü".repeat(MAX_TITLE_CHARS + 1));
    assert!(
        matches!(long, Err(Error::TooLarge { size, max, .. })
            if (size, max) == (MAX_TITLE_CHARS + 1, MAX_TITLE_CHARS)),
        "{long:?}"
    );
    bad_title("two\nlines", NO_CONTROL);
    // A title round-trips as a plain string.
    let value = title("Draft SPA");
    assert_eq!(serde_json::to_string(&value).unwrap(), r#""Draft SPA""#);
    assert_eq!(value.as_str().parse::<Title>().unwrap(), value);
}

#[test]
fn title_visibility() {
    ok_title("a");
    ok_title("\u{200B}a\u{FEFF}");
    for invisible in [
        "",
        " ",
        "\u{200B}",
        "\u{200C}\u{200D}",
        " \u{2060}\u{FEFF} ",
    ] {
        bad_title(invisible, NO_VISIBLE);
    }
    for bidi in [
        '\u{202A}', '\u{202E}', '\u{2066}', '\u{2069}', '\u{202B}', '\u{2067}',
    ] {
        bad_title(&format!("a{bidi}b"), NO_CONTROL);
    }
    ok_title("a\u{2065}b");
    ok_title("a\u{202F}b");
    ok_title("a b");
    for refused in ["a\u{0}b", "a\u{7F}b", "a\u{2028}b", "a\u{2029}b"] {
        bad_title(refused, NO_CONTROL);
    }
    // Bidi marks are allowed inside text.
    for mark in ['\u{200E}', '\u{200F}', '\u{061C}'] {
        ok_title(&format!("\u{5e9}{mark}1"));
    }
}

#[test]
fn invisible_characters_alone_are_refused() {
    let invisible = [
        '\u{200E}',
        '\u{200F}',
        '\u{061C}',
        '\u{00AD}',
        '\u{3164}',
        '\u{115F}',
        '\u{1160}',
        '\u{FFA0}',
        '\u{2800}',
        '\u{FE0F}',
        '\u{180E}',
        '\u{034F}',
        '\u{E0020}',
        '\u{E0041}',
        '\u{E007F}',
        '\u{E0001}',
        '\u{E0100}',
        '\u{17B4}',
        '\u{1D173}',
    ];
    for c in invisible {
        bad_title(&c.to_string(), NO_VISIBLE);
        bad_title(&format!(" {c}"), NO_VISIBLE);
        ok_title(&format!("{c}x"));
    }
    // A combining mark shows only on a character before it.
    let marks = [
        '\u{0301}', '\u{1AB0}', '\u{1DC0}', '\u{20D0}', '\u{FE20}', '\u{0483}', '\u{05B8}',
        '\u{064E}', '\u{0E31}', '\u{0E48}',
    ];
    for mark in marks {
        bad_title(&mark.to_string(), NO_VISIBLE);
        bad_title(&format!("{mark}{mark}\u{200B}"), NO_VISIBLE);
        ok_title(&format!("e{mark}"));
    }
    // Letters with marks show: a pointed Hebrew word, a vowelled Arabic one, Thai with a tone.
    for word in [
        "\u{5E9}\u{5C1}\u{5B8}\u{5DC}\u{5D5}\u{5B9}\u{5DD}",
        "\u{643}\u{64E}\u{62A}\u{64E}\u{628}\u{64E}",
        "\u{E44}\u{E21}\u{E48}",
    ] {
        ok_title(word);
    }
    // A tag sequence with no base character (a flag's tags alone) shows nothing.
    bad_title(
        "\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}",
        NO_VISIBLE,
    );
    // Prompts count visibility the same way.
    let builders: [fn(Vec<PromptBlock>) -> HostCommand; 2] = [send_prompt, edit_queued];
    for build in builders {
        assert!(invalid(&build(vec![text("\u{200B}")]).validate()));
        assert!(invalid(
            &build(vec![text("\u{3164}\u{00AD}\u{2800}")]).validate()
        ));
        assert!(
            build(vec![text("\u{200B}"), files(1).remove(0)])
                .validate()
                .is_ok()
        );
        assert!(build(vec![text("\u{200B}x")]).validate().is_ok());
    }
}

#[test]
fn agent_choice_ids_are_bounded_on_decode() {
    assert!(AgentChoiceId::new(&"m".repeat(MAX_ID_TEXT_BYTES)).is_ok());
    assert!(refused(&AgentChoiceId::new(
        &"m".repeat(MAX_ID_TEXT_BYTES + 1)
    )));
    assert!(refused(&AgentChoiceId::new("")));
    // Each field holding one refuses a bad id when the command decodes.
    let commands = [
        HostCommand::SetMode {
            thread: thread_id(1),
            mode_id: choice("plan"),
        },
        HostCommand::SetConfigOption {
            thread: thread_id(1),
            option_id: choice("plan"),
            value: ConfigValue::Boolean(true),
        },
        HostCommand::SetConfigOption {
            thread: thread_id(1),
            option_id: choice("model"),
            value: ConfigValue::Select(choice("plan")),
        },
    ];
    let long = format!(r#""{}""#, "m".repeat(MAX_ID_TEXT_BYTES + 1));
    for command in commands {
        let envelope = CommandEnvelope {
            command_id: command_id(1),
            command,
        };
        let json = serde_json::to_string(&envelope).unwrap();
        assert!(
            serde_json::from_str::<CommandEnvelope>(&json).is_ok(),
            "{json}"
        );
        for bad in [r#""""#, long.as_str()] {
            let json = json.replace(r#""plan""#, bad);
            err_contains(
                serde_json::from_str::<CommandEnvelope>(&json),
                "expected 1 to 256 UTF-8 bytes",
            );
        }
    }
}

#[test]
fn new_folder_name_rules() {
    let ok = |name: &str| assert!(new_folder(name).validate().is_ok(), "{name:?} refused");
    let bad = |name: &str| assert!(invalid(&new_folder(name).validate()), "{name:?} accepted");
    // Bytes: 255 pass, 256 fail (3-byte characters, within 200 characters).
    ok(&"€".repeat(85));
    let long = new_folder(&("€".repeat(85) + "a")).validate();
    assert!(too_large(&long, 256, 255), "{long:?}");
    for c in ['<', '>', ':', '"', '/', '\\', '|', '?', '*'] {
        bad(&format!("a{c}b"));
    }
    ok("Smith v Jones");
    ok("a.b c-d_(1)");
    bad("trailing.");
    bad("trailing ");
    bad(" leading");
    ok("inner. space");
    bad(".");
    bad("..");
    ok("a.hidden");
    for reserved in [
        "CON",
        "con",
        "Nul.txt",
        "com1",
        "LPT9.doc",
        "aux.tar.gz",
        "CON .txt",
    ] {
        bad(reserved);
    }
    for near in ["CONSOLE", "com10", "lpt00", "my con", "nul_x"] {
        ok(near);
    }
}

#[test]
fn dot_names_have_their_own_rule() {
    // `.` and `..` fail the leading and trailing dot rules too, so only the message tells the
    // rules apart.
    let rule_of = |name: &str| match new_folder(name).validate() {
        Err(Error::Format { rule, .. }) => rule,
        other => panic!("{name:?}: {other:?}"),
    };
    for name in [".", ".."] {
        assert_eq!(
            rule_of(name),
            "a new folder name is not `.` or `..`",
            "{name:?}"
        );
    }
    // A leading dot hides the folder in Finder.
    for name in [".hidden", "...a", ". x", ".CON"] {
        assert_eq!(
            rule_of(name),
            "a new folder name does not start with a dot, which hides it in Finder",
            "{name:?}"
        );
    }
}

#[test]
fn title_displays_its_text() {
    assert_eq!(title("Smith v Jones").to_string(), "Smith v Jones");
}

#[test]
fn existing_folder_name_is_a_label() {
    // Only the title rules: a Finder folder's name stays as it is.
    for (name, rule) in [
        ("Smith v Jones?", "holds none of < > : \" / \\ | ? *"),
        ("CON", "is not a Windows device name"),
        ("a:b", "holds none of < > : \" / \\ | ? *"),
        ("trailing.", "does not end with a dot or a space"),
        ("Q3/Q4", "holds none of < > : \" / \\ | ? *"),
    ] {
        let command = existing_folder(name);
        assert!(command.validate().is_ok(), "{name:?} refused");
        err_contains(new_folder(name).validate(), rule);
    }
    // 200 characters of 3 bytes: over the folder byte limit, within the title limit.
    assert!(existing_folder(&"€".repeat(200)).validate().is_ok());
    // The title rules still apply on decode.
    assert!(refused(&Title::new("a\u{202E}b")));
    assert!(refused(&Title::new("\u{200B}")));
}

#[test]
fn windows_reserved_names() {
    let reserved = [
        "CON",
        "con",
        "PRN",
        "aux",
        "NUL",
        "COM0",
        "com9",
        "LPT0",
        "lpt9",
        "lpt0.txt",
        "CONIN$",
        "conout$.log",
        "COM¹",
        "com².doc",
        "COM³",
        "LPT¹",
        "lpt²",
        "LPT³.x",
        "CON .txt",
        "nul .x",
        "con.",
        "Con  ",
        "aux.tar.gz",
    ];
    for name in reserved {
        assert!(is_windows_reserved_name(name), "{name:?} not reserved");
    }
    let allowed = [
        "", "CONSOLE", "com10", "lpt00", "COM⁴", "CONIN", "CONOUT$X", "LPT¹0", "com0x", " CON",
        "my con", "nul_x",
    ];
    for name in allowed {
        assert!(!is_windows_reserved_name(name), "{name:?} reserved");
    }
}
