//! The MCP server confirm: its words in both languages, how a program and an address show
//! (quoted, written out, shortened), and the answer: only Allow allows.

use std::sync::Mutex;

use clauseo_host::{McpLaunchAsk, McpLaunchConfirm as _};
use clauseo_protocol::Language;
use futures::channel::oneshot;

use super::words::{EN, HI, MAX_SHOWN, shortened, shown_address, shown_program, table};
use super::{BoxFuture, LaunchConfirm, LaunchDrawer, LaunchWords};
use crate::dialogs::words::OFF_VOICE;

fn local(program: &[&str]) -> McpLaunchAsk {
    McpLaunchAsk {
        server: "files".to_owned(),
        program: program.iter().map(|&part| part.to_owned()).collect(),
        url: None,
    }
}

fn remote(url: &str) -> McpLaunchAsk {
    McpLaunchAsk {
        server: "search".to_owned(),
        program: Vec::new(),
        url: Some(url.to_owned()),
    }
}

fn parts(program: &[&str]) -> Vec<String> {
    program.iter().map(|&part| part.to_owned()).collect()
}

#[test]
fn mcp_local_words_in_both_languages() {
    let ask = local(&["npx", "-y", "files-mcp"]);
    let en = LaunchWords::new(&ask, Language::En);
    assert_eq!(en.title, "Let Clauseo run this program?");
    assert_eq!(
        en.message,
        "“npx -y files-mcp” will start when you work in a matter. Only allow programs you trust."
    );
    assert_eq!((en.ok.as_str(), en.cancel.as_str()), ("Allow", "Cancel"));
    let hi = LaunchWords::new(&ask, Language::Hi);
    assert_eq!(hi.title, "Clauseo को यह प्रोग्राम चलाने दें?");
    assert_eq!(
        hi.message,
        "जब आप किसी मामले में काम करेंगे, तब “npx -y files-mcp” शुरू होगा। केवल उन्हीं प्रोग्राम को \
         अनुमति दें जिन पर आपको भरोसा है।"
    );
    assert_eq!((hi.ok.as_str(), hi.cancel.as_str()), ("अनुमति दें", "रद्द करें"));
}

#[test]
fn mcp_remote_words_in_both_languages() {
    let ask = remote("https://tools.example/mcp");
    let en = LaunchWords::new(&ask, Language::En);
    assert_eq!(en.title, "Let Clauseo connect to this tool?");
    assert_eq!(
        en.message,
        "Clauseo will connect to “https://tools.example/mcp” when you work in a matter. Only \
         allow tools you trust."
    );
    assert_eq!((en.ok.as_str(), en.cancel.as_str()), ("Allow", "Cancel"));
    let hi = LaunchWords::new(&ask, Language::Hi);
    assert_eq!(hi.title, "Clauseo को इस टूल से जुड़ने दें?");
    assert_eq!(
        hi.message,
        "जब आप किसी मामले में काम करेंगे, तब Clauseo “https://tools.example/mcp” से जुड़ेगा। केवल \
         उन्हीं टूल को अनुमति दें जिन पर आपको भरोसा है।"
    );
    assert_eq!((hi.ok.as_str(), hi.cancel.as_str()), ("अनुमति दें", "रद्द करें"));
}

#[test]
fn mcp_words_keep_the_apps_voice() {
    for table in [&EN, &HI] {
        let all = [
            table.local_title,
            table.local_message,
            table.ok,
            table.cancel,
            table.remote_title,
            table.remote_message,
        ];
        for words in all {
            assert!(!words.contains('!'), "{words}");
            assert_eq!(words.trim(), words);
            assert!(!words.contains("  "), "{words}");
            let lower = words.to_lowercase();
            for word in OFF_VOICE {
                assert!(!lower.contains(word), "{words}");
            }
        }
    }
    assert!(table(Language::Hi).local_message.contains("{program}"));
    assert!(table(Language::Hi).remote_message.contains("{address}"));
}

#[test]
fn mcp_cancel_comes_first() {
    for words in LaunchWords::every_table_for_tests() {
        let rfd::MessageButtons::OkCancelCustom(first, second) =
            crate::dialogs::cancel_first(&words.ok, &words.cancel)
        else {
            panic!("not Cancel first");
        };
        assert_eq!((first, second), (words.cancel.clone(), words.ok.clone()));
        assert_ne!(words.ok, words.cancel);
    }
}

#[test]
fn mcp_program_quotes_as_a_shell_would() {
    assert_eq!(
        shown_program(&parts(&["uvx", "mcp-server"])),
        "uvx mcp-server"
    );
    assert_eq!(
        shown_program(&parts(&[
            "/Applications/My Tools/run",
            "--dir",
            "C:\\Case Files",
            ""
        ])),
        "'/Applications/My Tools/run' --dir 'C:\\Case Files' ''"
    );
    assert_eq!(
        shown_program(&parts(&["node", "it's here", "say \"hi\""])),
        "node \"it's here\" 'say \"hi\"'"
    );
    assert_eq!(shown_program(&[]), "");
}

#[test]
fn mcp_program_writes_out_what_would_hide_or_add_lines() {
    assert_eq!(
        shown_program(&parts(&["run", "a\nOnly allow"])),
        "run 'a\\nOnly allow'"
    );
    assert_eq!(
        shown_program(&parts(&["run", "x\u{202e}gpj"])),
        "run x\\u{202e}gpj"
    );
    assert_eq!(
        shown_address("https://a.example/\u{200b}"),
        "https://a.example/\\u{200b}"
    );
}

#[test]
fn mcp_slots_fill_once() {
    let words = LaunchWords::new(&local(&["run", "{address}", "{program}"]), Language::En);
    assert!(
        words
            .message
            .starts_with("“run {address} {program}” will start"),
        "{}",
        words.message
    );
    let words = LaunchWords::new(&remote("https://a.example/{program}"), Language::Hi);
    assert!(
        words.message.contains("“https://a.example/{program}”"),
        "{}",
        words.message
    );
}

#[test]
fn mcp_long_program_loses_its_middle() {
    let long = "a".repeat(400);
    let shown = shown_program(&parts(&["start", &long, "end"]));
    assert_eq!(shown.chars().count(), MAX_SHOWN);
    assert!(shown.starts_with("start aaa"), "{shown}");
    assert!(shown.ends_with("aaa end"), "{shown}");
    assert_eq!(shown.matches('…').count(), 1);
    // short enough: as it is
    let exact = "b".repeat(MAX_SHOWN);
    assert_eq!(shortened(&exact), exact);
    // cut by characters, never inside one
    let wide = "मामला🙂".repeat(100);
    let shown = shortened(&wide);
    assert_eq!(shown.chars().count(), MAX_SHOWN);
    assert!(shown.starts_with("मामला🙂") && shown.contains('…'));
    let address = shown_address(&format!("https://a.example/{}", "p".repeat(500)));
    assert_eq!(address.chars().count(), MAX_SHOWN);
    assert!(address.starts_with("https://a.example/"));
}

/// How the fake drawer answers.
#[derive(Clone, Copy)]
enum Answer {
    Allow,
    Cancel,
    /// The dialog went away unanswered: its sender dropped, as `dialogs::confirm`'s does.
    Never,
}

struct FakeDrawer {
    language: Language,
    answer: Answer,
    drawn: Mutex<Vec<LaunchWords>>,
}

impl FakeDrawer {
    fn new(language: Language, answer: Answer) -> Self {
        Self {
            language,
            answer,
            drawn: Mutex::new(Vec::new()),
        }
    }
}

impl LaunchDrawer for FakeDrawer {
    fn language(&self) -> BoxFuture<'_, Language> {
        let language = self.language;
        Box::pin(async move { language })
    }

    fn draw(&self, words: LaunchWords) -> BoxFuture<'_, bool> {
        let (tx, rx) = oneshot::channel();
        match self.answer {
            Answer::Allow => tx.send(true).unwrap(),
            Answer::Cancel => tx.send(false).unwrap(),
            Answer::Never => drop(tx),
        }
        self.drawn.lock().unwrap().push(words);
        Box::pin(async move { rx.await.unwrap_or(false) })
    }
}

#[test]
fn mcp_only_allow_allows() {
    let ask = local(&["npx", "files-mcp"]);
    for (answer, allowed) in [
        (Answer::Allow, true),
        (Answer::Cancel, false),
        (Answer::Never, false),
    ] {
        let confirm = LaunchConfirm(FakeDrawer::new(Language::En, answer));
        assert_eq!(confirm.confirm(&ask), allowed);
        let drawn = confirm.0.drawn.lock().unwrap();
        assert_eq!(drawn.as_slice(), [LaunchWords::new(&ask, Language::En)]);
    }
}

#[test]
fn mcp_confirm_speaks_the_apps_language() {
    let confirm = LaunchConfirm(FakeDrawer::new(Language::Hi, Answer::Cancel));
    assert!(!confirm.confirm(&remote("https://tools.example/mcp")));
    let drawn = confirm.0.drawn.lock().unwrap();
    assert_eq!(drawn[0].title, "Clauseo को इस टूल से जुड़ने दें?");
}

/// Nothing here logs: the program, its arguments and the URL stay out of the log.
#[test]
fn mcp_confirm_logs_nothing() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in ["src/mcp.rs", "src/mcp/words.rs"] {
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        for call in ["tracing::", "log::", "println!", "eprintln!", "dbg!"] {
            assert!(!text.contains(call), "{file} calls {call}");
        }
    }
}

/// Every word here, in both languages, appears verbatim in its review home,
/// `ui/app/src/review/registry-uc-mcp-confirm.tsx`, under its id (UC-214 on, in the table's
/// order), so what the reviewers read is what Rust draws.
#[test]
fn mcp_words_are_the_reviewed_copy() {
    let registry = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/app/src/review/registry-uc-mcp-confirm.tsx");
    let registry = std::fs::read_to_string(&registry).unwrap();
    let column = |table: &super::words::Table| {
        [
            table.local_title,
            table.local_message,
            table.ok,
            table.cancel,
            table.remote_title,
            table.remote_message,
        ]
    };
    for (n, (en, hi)) in (214..).zip(column(&EN).into_iter().zip(column(&HI))) {
        let id = format!("\"UC-{n}\"");
        assert!(registry.contains(&id), "no {id}");
        for words in [en, hi] {
            let quoted = format!("\"{words}\"");
            assert!(registry.contains(&quoted), "{id} lacks {quoted}");
        }
    }
}
