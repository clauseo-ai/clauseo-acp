//! `cmd /c` or `/k` given its command line in one word (`cmd /c "del a.docx"`, as PowerShell
//! passes a quoted one): that word read as cmd reads a command line, split at blanks outside
//! `"`, the quotes taken off, so its delete or move reads as the words after `/c` do.

use super::Word;
use super::deletes::name_of;

/// How `cmd`'s arguments read.
pub(super) enum Inner {
    /// As they are: no `cmd /c` whose command line is one word with a blank.
    Whole,
    /// With the command line split into its words.
    Split(Vec<Word>),
    /// A command line this reading does not split: one with an expansion, or holding a cmd
    /// operator (`&`, `|`, `<`, `>`, `(`, `)`), `^` or `%`.
    Unread,
}

/// `args` of `program` with `cmd /c`'s command line split into its words, when that line is one
/// word holding a blank.
pub(super) fn split(program: &str, args: &[Word]) -> Inner {
    if name_of(program) != "cmd" {
        return Inner::Whole;
    }
    let Some(at) = args
        .iter()
        .position(|arg| matches!(arg.text.to_ascii_lowercase().as_str(), "/c" | "/k"))
    else {
        return Inner::Whole;
    };
    let Some(line) = args
        .get(at + 1)
        .filter(|word| word.text.contains(char::is_whitespace))
    else {
        return Inner::Whole;
    };
    if !line.clear || line.text.contains(['&', '|', '<', '>', '(', ')', '^', '%']) {
        return Inner::Unread;
    }
    let mut words: Vec<Word> = args.get(..=at).unwrap_or_default().to_vec();
    words.extend(line_words(&line.text));
    words.extend_from_slice(args.get(at + 2..).unwrap_or_default());
    Inner::Split(words)
}

/// The words of the command line `line`: split at blanks outside `"`, the quotes taken off; a
/// word holding `*` or `?` is no plain text.
fn line_words(line: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut text = String::new();
    let (mut quoted, mut started) = (false, false);
    for c in line.chars() {
        if c == '"' {
            quoted = !quoted;
            started = true;
        } else if c.is_whitespace() && !quoted {
            if std::mem::take(&mut started) {
                out.push(word(std::mem::take(&mut text)));
            }
        } else {
            text.push(c);
            started = true;
        }
    }
    if started {
        out.push(word(text));
    }
    out
}

fn word(text: String) -> Word {
    Word {
        clear: !text.contains(['*', '?']),
        text,
        list: false,
    }
}
