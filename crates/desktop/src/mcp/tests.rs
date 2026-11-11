//! The MCP server confirm: its words in both languages, how a program and an address show
//! (quoted, written out, shortened), and the answer: only Allow allows.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use clauseo_host::{McpLaunchAsk, McpLaunchConfirm as _};
use clauseo_protocol::Language;
use futures::channel::oneshot;

use super::words::{EN, HI, shown_address, shown_program, table};
use super::{BoxFuture, LaunchConfirm, LaunchDrawer, LaunchWords};
use crate::dialogs::words::OFF_VOICE;

fn local(program: &[&str]) -> McpLaunchAsk {
    McpLaunchAsk {
        server: "files".to_owned(),
        program: program.iter().map(|&part| part.to_owned()).collect(),
        url: None,
        names: Vec::new(),
    }
}

fn remote(url: &str) -> McpLaunchAsk {
    McpLaunchAsk {
        server: "search".to_owned(),
        program: Vec::new(),
        url: Some(url.to_owned()),
        names: Vec::new(),
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

/// Nothing is ever cut: padding a program to the host's limit (`MAX_ASK_CHARS`) leaves a dangerous
/// argument at its end in view, and a long URL shows whole.
#[test]
fn mcp_long_program_shows_whole() {
    let padding = "a".repeat(1_900);
    let shown = shown_program(&parts(&["npx", &padding, "--require=/tmp/x.js"]));
    assert!(shown.ends_with(" --require=/tmp/x.js"), "{shown}");
    assert!(!shown.contains('…'));
    let words = LaunchWords::new(
        &local(&["npx", &padding, "--require=/tmp/x.js"]),
        Language::En,
    );
    assert!(
        words.message.contains("--require=/tmp/x.js"),
        "{}",
        words.message
    );
    let url = format!("https://a.example/{}", "p".repeat(1_500));
    assert_eq!(shown_address(&url), url);
}

/// The line naming new or changed variables (a local server) or headers (a remote one) sits
/// under the message when there are names, joined with ", " verbatim, and not at all otherwise.
#[test]
fn mcp_names_line_shows_only_with_names() {
    for language in [Language::En, Language::Hi] {
        let table = table(language);
        let plain = LaunchWords::new(&local(&["npx", "files-mcp"]), language);
        assert!(!plain.message.contains("\n\n"), "{}", plain.message);
        let mut ask = local(&["npx", "files-mcp"]);
        ask.names = vec!["FILES_TOKEN".to_owned(), "NODE_OPTIONS".to_owned()];
        let named = LaunchWords::new(&ask, language);
        let line = table
            .local_names
            .replace("{names}", "FILES_TOKEN, NODE_OPTIONS");
        assert!(
            named.message.ends_with(&format!("\n\n{line}")),
            "{}",
            named.message
        );
        let mut ask = remote("https://tools.example/mcp");
        ask.names = vec!["Authorization".to_owned()];
        let named = LaunchWords::new(&ask, language);
        let line = table.remote_names.replace("{names}", "Authorization");
        assert!(
            named.message.ends_with(&format!("\n\n{line}")),
            "{}",
            named.message
        );
        let plain = LaunchWords::new(&remote("https://tools.example/mcp"), language);
        assert!(!plain.message.contains("\n\n"), "{}", plain.message);
    }
    assert_eq!(EN.local_names, "New or changed settings: {names}.");
    assert_eq!(EN.remote_names, "New or changed headers: {names}.");
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
        let confirm = LaunchConfirm::new(FakeDrawer::new(Language::En, answer));
        assert_eq!(confirm.confirm(&ask), allowed);
        let drawn = confirm.drawer().drawn.lock().unwrap();
        assert_eq!(drawn.as_slice(), [LaunchWords::new(&ask, Language::En)]);
    }
}

/// An import's one confirm: only Allow allows, and it draws the import's words.
#[test]
fn mcp_import_confirm_only_allow_allows() {
    let asks = [
        local(&["npx", "files-mcp"]),
        remote("https://tools.example/mcp"),
    ];
    for (answer, allowed) in [
        (Answer::Allow, true),
        (Answer::Cancel, false),
        (Answer::Never, false),
    ] {
        let confirm = LaunchConfirm(FakeDrawer::new(Language::En, answer));
        assert_eq!(confirm.confirm_import(&asks), allowed);
        let drawn = confirm.0.drawn.lock().unwrap();
        let words = super::import_words::import_words(&asks, Language::En);
        assert_eq!(drawn.as_slice(), [words]);
    }
}

#[test]
fn mcp_confirm_speaks_the_apps_language() {
    let confirm = LaunchConfirm::new(FakeDrawer::new(Language::Hi, Answer::Cancel));
    assert!(!confirm.confirm(&remote("https://tools.example/mcp")));
    let drawn = confirm.drawer().drawn.lock().unwrap();
    assert_eq!(drawn[0].title, "Clauseo को इस टूल से जुड़ने दें?");
}

/// Nothing here logs: the program, its arguments and the URL stay out of the log.
#[test]
fn mcp_confirm_logs_nothing() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in ["src/mcp.rs", "src/mcp/words.rs", "src/mcp/import_words.rs"] {
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
            table.local_names,
            table.remote_names,
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

/// A hand-moved clock for the flood control.
fn clock() -> (super::Clock, Arc<Mutex<Instant>>) {
    let at = Arc::new(Mutex::new(crate::open::now()));
    let read = Arc::clone(&at);
    (Arc::new(move || *read.lock().unwrap()), at)
}

/// After a Cancel the same ask within ten seconds is declined with no dialog; another ask, or
/// the same one later, draws one.
#[test]
fn mcp_a_cancelled_ask_is_not_asked_again_at_once() {
    let (now, at) = clock();
    let confirm = LaunchConfirm::with_clock(FakeDrawer::new(Language::En, Answer::Cancel), now);
    let ask = local(&["npx", "files-mcp"]);
    assert!(!confirm.confirm(&ask));
    assert!(!confirm.confirm(&ask));
    assert_eq!(
        confirm.drawer().drawn.lock().unwrap().len(),
        1,
        "a second dialog came"
    );
    assert!(!confirm.confirm(&local(&["npx", "other-mcp"])));
    assert_eq!(confirm.drawer().drawn.lock().unwrap().len(), 2);
    *at.lock().unwrap() += super::SAME_ASK_QUIET;
    assert!(!confirm.confirm(&ask));
    assert_eq!(confirm.drawer().drawn.lock().unwrap().len(), 3);
}

/// A drawer whose dialog stays open until the test answers it.
struct Held {
    shown: std::sync::mpsc::Sender<()>,
    answer: Mutex<Option<oneshot::Receiver<bool>>>,
}

impl LaunchDrawer for Held {
    fn language(&self) -> BoxFuture<'_, Language> {
        Box::pin(async { Language::En })
    }

    fn draw(&self, _words: LaunchWords) -> BoxFuture<'_, bool> {
        let rx = self.answer.lock().unwrap().take();
        self.shown.send(()).unwrap();
        Box::pin(async move {
            match rx {
                Some(rx) => rx.await.unwrap_or(false),
                None => false,
            }
        })
    }
}

/// While one dialog is open, another ask is declined at once, without a second dialog.
#[test]
fn mcp_one_dialog_at_a_time() {
    let (shown_tx, shown_rx) = std::sync::mpsc::channel();
    let (answer_tx, answer_rx) = oneshot::channel();
    let confirm = Arc::new(LaunchConfirm::new(Held {
        shown: shown_tx,
        answer: Mutex::new(Some(answer_rx)),
    }));
    let first = {
        let confirm = Arc::clone(&confirm);
        std::thread::spawn(move || confirm.confirm(&local(&["npx", "files-mcp"])))
    };
    shown_rx.recv().unwrap();
    assert!(
        !confirm.confirm(&local(&["npx", "other-mcp"])),
        "a second dialog was drawn"
    );
    assert!(shown_rx.try_recv().is_err());
    answer_tx.send(true).unwrap();
    assert!(first.join().unwrap());
}
