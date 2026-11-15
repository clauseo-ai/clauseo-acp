//! The save confirm's cap counts the text as the confirm shows it: an argument that grows when
//! quoted or escaped is refused once its shown text passes [`MAX_ASK_CHARS`], even when its raw
//! text is far shorter; one that shows at exactly the cap asks, and shows whole.

use std::collections::BTreeMap;

use clauseo_protocol::{McpServerEdit, McpServerKind};
use clauseo_settings::McpServerLaunch;

use super::{MAX_ASK_CHARS, ask, shown};

const NPX: &str = "npx";

fn edit(args: Vec<String>) -> McpServerEdit {
    McpServerEdit {
        name: "files".to_owned(),
        kind: McpServerKind::Local,
        command: Some(NPX.to_owned()),
        args,
        url: None,
        env: Vec::new(),
        headers: Vec::new(),
        enabled: true,
    }
}

fn launch(args: &[String]) -> McpServerLaunch {
    McpServerLaunch::local(NPX.to_owned(), args.to_vec(), BTreeMap::new(), true)
}

/// The shown program of the ask for adding a server running `npx` with `args`, or the refusal
/// of one too long to show. What is asked is what is shown: every argument, whole and verbatim.
fn asks(args: Vec<String>) -> Result<String, crate::error::Error> {
    let one = ask(None, &launch(&args), &edit(args.clone()))?.expect("an add asks");
    assert_eq!(one.program[1..], args[..]);
    Ok(shown::program(&one.program))
}

/// `npx ` and an argument whose shown text is `shown_len` characters.
fn at(shown_len: usize, part: &str, part_shown: usize, frame: usize) -> Vec<String> {
    let repeats = (shown_len - NPX.len() - 1 - frame) / part_shown;
    vec![part.repeat(repeats)]
}

#[test]
fn control_characters_count_as_their_escapes_at_the_cap() {
    // A newline shows as `\n`: two characters, and no quotes (the shown text holds no space).
    let fits = at(MAX_ASK_CHARS, "\n", 2, 0);
    let shown = asks(fits.clone()).unwrap();
    assert_eq!(shown.chars().count(), MAX_ASK_CHARS);
    assert!(!shown.contains('\n'));
    // One more newline: 2,002 shown, though the raw text is barely past half the cap.
    let over = vec![format!("{}\n", fits[0])];
    assert!(NPX.len() + over[0].len() < MAX_ASK_CHARS / 2 + 10);
    assert!(asks(over).is_err());
}

#[test]
fn quotes_and_backslashes_count_as_shown_at_the_cap() {
    // `it's` puts the argument in double quotes, and each `\` shows as `\\`.
    let mut arg = "it's".to_owned();
    // `npx "it's` and the closing quote are 10 characters; each backslash then shows as two.
    arg.push_str(&"\\".repeat((MAX_ASK_CHARS - 10) / 2));
    let shown = asks(vec![arg.clone()]).unwrap();
    assert_eq!(shown.chars().count(), MAX_ASK_CHARS);
    assert!(shown.ends_with("\\\\\""));
    arg.push('\\');
    assert!(asks(vec![arg]).is_err());
}

#[test]
fn long_letters_count_as_one_character_each_at_the_cap() {
    // Four bytes each in UTF-8, one character each on screen.
    let fits = at(MAX_ASK_CHARS, "𝔄", 1, 0);
    assert_eq!(asks(fits.clone()).unwrap().chars().count(), MAX_ASK_CHARS);
    let over = vec![format!("{}𝔄", fits[0])];
    assert!(asks(over).is_err());
}

#[test]
fn hidden_marks_count_as_their_escapes_and_a_space_adds_quotes() {
    // A right-to-left override shows as `\u{202e}` (8 characters); a space adds two quotes.
    let mut arg = " ".to_owned();
    let marks = (MAX_ASK_CHARS - NPX.len() - 1 - 3) / 8;
    arg.push_str(&"\u{202e}".repeat(marks));
    let shown = asks(vec![arg.clone()]).unwrap();
    assert!(shown.chars().count() <= MAX_ASK_CHARS);
    assert!(shown.chars().count() > MAX_ASK_CHARS - 8);
    arg.push_str("\u{202e}");
    assert!(asks(vec![arg]).is_err());
}
