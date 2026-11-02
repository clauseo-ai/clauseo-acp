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

/// A shell command's words, grouped by the simple command (segment) each belongs to, the first
/// `limit` words in all.
#[derive(Debug)]
pub(crate) struct Segments {
    /// The segments in order, each its words; empty segments are not kept.
    list: Vec<Vec<String>>,
    /// How many words `list` holds.
    count: usize,
    /// The most words kept.
    limit: usize,
}

impl Segments {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            list: Vec::new(),
            count: 0,
            limit,
        }
    }

    /// Whether it holds `limit` words.
    pub(crate) fn full(&self) -> bool {
        self.count >= self.limit
    }

    /// Adds `word` to the last segment, unless it is full.
    pub(crate) fn push(&mut self, word: String) {
        if self.full() {
            return;
        }
        match self.list.last_mut() {
            Some(last) => last.push(word),
            None => self.list.push(vec![word]),
        }
        self.count += 1;
    }

    /// Ends the last segment: the next word starts a new one.
    pub(crate) fn split(&mut self) {
        if self.list.last().is_none_or(|last| !last.is_empty()) {
            self.list.push(Vec::new());
        }
    }

    /// The segments, each its words, empty ones left out.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &[String]> {
        self.list
            .iter()
            .filter(|segment| !segment.is_empty())
            .map(Vec::as_slice)
    }
}

/// Adds `command`'s words to `segments` until it is full: split at unquoted whitespace;
/// single quotes keep everything; double quotes keep everything but the escape before `"`, the
/// escape itself, `$` or `` ` `` (a backslash under [`Escape::Backslash`]), and the escape
/// outside quotes keeps the next character. No expansion. A new segment starts at each unquoted
/// newline, `;`, `&&`, `|` and `||` (`|&` too), which end the word before them; a `|` right
/// after `>` is the redirection `>|` and stays in the word, as does a single `&` (`2>&1`,
/// `&>x`, a trailing `&`). An unclosed quote ends with the command.
pub(crate) fn shell_segments(command: &str, escape: Escape, segments: &mut Segments) {
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
    segments.split();
    let end_word = |word: &mut String, in_word: &mut bool, segments: &mut Segments| {
        if *in_word {
            segments.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        if segments.full() {
            return;
        }
        match (quote, c) {
            (Quote::None, '\n' | ';') => {
                end_word(&mut word, &mut in_word, segments);
                segments.split();
            }
            (Quote::None, '&') if chars.peek() == Some(&'&') => {
                chars.next();
                end_word(&mut word, &mut in_word, segments);
                segments.split();
            }
            (Quote::None, '|') if !word.ends_with('>') => {
                if matches!(chars.peek(), Some('|' | '&')) {
                    chars.next();
                }
                end_word(&mut word, &mut in_word, segments);
                segments.split();
            }
            (Quote::None, c) if c.is_whitespace() => {
                end_word(&mut word, &mut in_word, segments);
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
    end_word(&mut word, &mut in_word, segments);
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
    use super::{Escape, Segments, candidates, shell_segments};

    fn segments(command: &str, limit: usize, escape: Escape) -> Vec<Vec<String>> {
        let mut segments = Segments::new(limit);
        shell_segments(command, escape, &mut segments);
        segments.iter().map(<[String]>::to_vec).collect()
    }

    fn split(command: &str, limit: usize, escape: Escape) -> Vec<String> {
        segments(command, limit, escape).concat()
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
    fn commands_split_into_segments_at_unquoted_operators() {
        let posix = Escape::Backslash;
        assert_eq!(
            segments(
                "mkdir -p D && cd D;echo 'a;b' \"c && d\" >|x | tee y||z\nw 2>&1 & v |& u",
                64,
                posix
            ),
            [
                vec!["mkdir", "-p", "D"],
                vec!["cd", "D"],
                vec!["echo", "a;b", "c && d", ">|x"],
                vec!["tee", "y"],
                vec!["z"],
                vec!["w", "2>&1", "&", "v"],
                vec!["u"],
            ]
        );
        assert_eq!(
            segments("a\\;b ; ;; c", 64, posix),
            [vec!["a;b"], vec!["c"]]
        );
        assert_eq!(segments("a; b; c", 2, posix), [vec!["a"], vec!["b"]]);
        assert_eq!(
            segments(
                "Set-Location D; Set-Content ..\\C.docx hi",
                64,
                Escape::Backtick
            ),
            [
                vec!["Set-Location", "D"],
                vec!["Set-Content", r"..\C.docx", "hi"]
            ]
        );
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
