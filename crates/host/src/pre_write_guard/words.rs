//! The shell side of the guard's heuristic: a command's words, the simple shell way, and the
//! file names a word may hold.

/// Which character escapes the next one in a shell command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Escape {
    /// POSIX shells (`Bash`): a backslash.
    Backslash,
    /// `PowerShell`: a backtick; a backslash is a plain character (a Windows path's separator).
    Backtick,
}

/// Appends `command`'s words to `words` until it holds `limit`: split at unquoted whitespace;
/// single quotes keep everything; double quotes keep everything but the escape before `"`, the
/// escape itself, `$` or `` ` `` (a backslash under [`Escape::Backslash`]), and the escape
/// outside quotes keeps the next character. No expansion, no operators: `a;b` is one word. An
/// unclosed quote ends with the command.
pub(crate) fn shell_words(command: &str, limit: usize, escape: Escape, words: &mut Vec<String>) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }
    let escape = match escape {
        Escape::Backslash => '\\',
        Escape::Backtick => '`',
    };
    let mut quote = Quote::None;
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        if words.len() >= limit {
            return;
        }
        match (quote, c) {
            (Quote::None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            (Quote::None, '\'') => {
                quote = Quote::Single;
                in_word = true;
            }
            (Quote::None, '"') => {
                quote = Quote::Double;
                in_word = true;
            }
            (Quote::None, c) if c == escape => {
                in_word = true;
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            (Quote::Single, '\'') | (Quote::Double, '"') => quote = Quote::None,
            (Quote::Double, c) if c == escape => match chars.peek() {
                Some(&next) if matches!(next, '"' | '$' | '`') || next == escape => {
                    word.push(next);
                    chars.next();
                }
                _ => word.push(escape),
            },
            (Quote::None | Quote::Single | Quote::Double, c) => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word && words.len() < limit {
        words.push(word);
    }
}

/// The file names `word` may hold, each when not empty: the word without trailing `;`, `|` or
/// `&` (`Contract.docx;`), its part after the last `>`, `<` or `|` (a redirection such as
/// `>Contract.docx`, `>>x`, `>|x` or `2>x`), its part after the first `=`
/// (`--out=Contract.docx`), and for a word starting with `-` its part after the first `:`
/// without quotes around it (PowerShell's colon form of a parameter, `-Path:Contract.docx`).
pub(crate) fn candidates(word: &str) -> impl Iterator<Item = &str> {
    let word = word.trim_end_matches([';', '|', '&']);
    let redirected = word
        .rfind(['>', '<', '|'])
        .and_then(|at| word.get(at + 1..));
    let assigned = word.split_once('=').map(|(_, value)| value);
    let parameter = word
        .strip_prefix('-')
        .and_then(|rest| rest.split_once(':'))
        .map(|(_, value)| value.trim_matches(['\'', '"']));
    [Some(word), redirected, assigned, parameter]
        .into_iter()
        .flatten()
        .filter(|candidate| !candidate.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{Escape, candidates, shell_words};

    fn split(command: &str, limit: usize, escape: Escape) -> Vec<String> {
        let mut words = Vec::new();
        shell_words(command, limit, escape, &mut words);
        words
    }

    #[test]
    fn posix_words_split_the_simple_shell_way() {
        let posix = Escape::Backslash;
        assert_eq!(
            split(r#"cp  'a b' "c \"d\" \$e \x" f\ g '' h"#, 64, posix),
            ["cp", "a b", r#"c "d" $e \x"#, "f g", "", "h"]
        );
        assert_eq!(split("a 'b c", 64, posix), ["a", "b c"]);
        assert_eq!(split("a b c d", 2, posix), ["a", "b"]);
        assert_eq!(split("trailing\\", 64, posix), ["trailing"]);
        assert_eq!(split("  \t\n ", 64, posix), Vec::<String>::new());
    }

    #[test]
    fn powershell_words_keep_backslashes_and_escape_with_backticks() {
        let ps = Escape::Backtick;
        assert_eq!(
            split(
                r#"Set-Content C:\m\Contract.docx "a `"b`" \x" a` b"#,
                64,
                ps
            ),
            ["Set-Content", r"C:\m\Contract.docx", r#"a "b" \x"#, "a b"]
        );
    }

    #[test]
    fn candidates_drop_shell_punctuation_around_a_name() {
        fn all(word: &str) -> Vec<&str> {
            candidates(word).collect()
        }
        assert_eq!(all("Contract.docx;"), ["Contract.docx"]);
        assert_eq!(all("Contract.docx&&"), ["Contract.docx"]);
        assert_eq!(all(">Contract.docx"), [">Contract.docx", "Contract.docx"]);
        assert_eq!(all(">>Contract.docx"), [">>Contract.docx", "Contract.docx"]);
        assert_eq!(all(">|Contract.docx"), [">|Contract.docx", "Contract.docx"]);
        assert_eq!(all("2>Contract.docx"), ["2>Contract.docx", "Contract.docx"]);
        assert_eq!(
            all("--out=Contract.docx"),
            ["--out=Contract.docx", "Contract.docx"]
        );
        assert_eq!(
            all("-Path:Contract.docx"),
            ["-Path:Contract.docx", "Contract.docx"]
        );
        assert_eq!(
            all(r#"-LiteralPath:"C:\m\Contract.docx""#),
            [
                r#"-LiteralPath:"C:\m\Contract.docx""#,
                r"C:\m\Contract.docx"
            ]
        );
        assert_eq!(all("a:b"), ["a:b"]);
        assert_eq!(all("-Force:"), ["-Force:"]);
        assert_eq!(all(";"), Vec::<&str>::new());
    }
}
