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

/// One simple command's words, or a heredoc body's (`command` false: its lines are data, not
/// commands, but their words still name files).
#[derive(Debug, Default)]
pub(crate) struct Segment {
    pub(crate) words: Vec<String>,
    pub(crate) command: bool,
}

/// A shell command's words, grouped by the simple command (segment) each belongs to, the first
/// `limit` words in all.
#[derive(Debug)]
pub(crate) struct Segments {
    /// The segments in order; the last may be empty.
    list: Vec<Segment>,
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
        if self.list.is_empty() {
            self.start(true);
        }
        if let Some(last) = self.list.last_mut() {
            last.words.push(word);
            self.count += 1;
        }
    }

    /// Ends the last segment: the next word starts a new command.
    pub(crate) fn split(&mut self) {
        self.start(true);
    }

    /// Starts a new segment, a command or a heredoc body.
    fn start(&mut self, command: bool) {
        match self.list.last_mut() {
            Some(last) if last.words.is_empty() => last.command = command,
            _ => self.list.push(Segment {
                words: Vec::new(),
                command,
            }),
        }
    }

    /// The segments, empty ones left out.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Segment> {
        self.list.iter().filter(|segment| !segment.words.is_empty())
    }
}

/// Adds `command`'s words to `segments` until it is full, the simple shell way, with no
/// expansion:
///
/// - Words split at unquoted whitespace. Single quotes keep everything; double quotes keep
///   everything but the escape before `"`, the escape itself, `$`, `` ` `` or a newline (a
///   backslash under [`Escape::Backslash`]); the escape outside quotes keeps the next character;
///   an escaped newline joins the lines. Under [`Escape::Backslash`], `$'…'` is read as its
///   quoted text with the basic escapes (`\n`, `\t`, `\\`, `\'`, …). An unclosed quote ends with
///   the command.
/// - An unquoted `#` at a word's start begins a comment, to the end of the line.
/// - Unquoted `<` and `>` end a word and are dropped (`cat>Contract.docx<x` is three words), as
///   is an `&` or `|` that belongs to a redirection (`2>&1`, `&>x`, `>|x`).
/// - A new segment starts at each unquoted newline, `;`, `&`, `&&`, `|`, `||`, `|&`, `(` and
///   `)`.
/// - A heredoc (`<<WORD`, `<<-WORD`, quoted or not) has its body, the lines after the command's
///   line up to the one that is `WORD`, put in a segment of its own that is not a command.
pub(crate) fn shell_segments(command: &str, escape: Escape, segments: &mut Segments) {
    let escape = match escape {
        Escape::Backslash => '\\',
        Escape::Backtick => '`',
    };
    let mut scanner = Scanner {
        chars: command.chars().peekable(),
        escape,
        ansi_c: escape == '\\',
        word: String::new(),
        in_word: false,
        awaiting_delimiter: None,
        heredocs: Vec::new(),
    };
    segments.split();
    scanner.run(segments);
}

/// The state of [`shell_segments`].
struct Scanner<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    escape: char,
    /// Whether `$'…'` is ANSI-C quoting (POSIX shells).
    ansi_c: bool,
    word: String,
    in_word: bool,
    /// After `<<` or `<<-`: the next word is a heredoc's delimiter; whether tabs are stripped.
    awaiting_delimiter: Option<bool>,
    /// Heredocs whose bodies start after this line: delimiter, tabs stripped.
    heredocs: Vec<(String, bool)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    None,
    Single,
    Double,
    AnsiC,
}

impl Scanner<'_> {
    fn run(&mut self, segments: &mut Segments) {
        let mut quote = Quote::None;
        let mut prev = ' ';
        while let Some(c) = self.chars.next() {
            if segments.full() {
                return;
            }
            let unquoted = quote == Quote::None;
            match (quote, c) {
                (Quote::None, '#') if !self.in_word => {
                    while self.chars.peek().is_some_and(|&next| next != '\n') {
                        self.chars.next();
                    }
                }
                (Quote::None, '\n') => {
                    self.end_word(segments);
                    segments.split();
                    self.bodies(segments);
                }
                (Quote::None, ';' | '(' | ')') => {
                    self.end_word(segments);
                    segments.split();
                }
                (Quote::None, '&') => {
                    self.end_word(segments);
                    if self.chars.peek() == Some(&'&') {
                        self.chars.next();
                        segments.split();
                    } else if !matches!(prev, '>' | '<') && self.chars.peek() != Some(&'>') {
                        segments.split();
                    }
                }
                (Quote::None, '|') => {
                    self.end_word(segments);
                    if prev != '>' {
                        if matches!(self.chars.peek(), Some('|' | '&')) {
                            self.chars.next();
                        }
                        segments.split();
                    }
                }
                (Quote::None, '>') => self.end_word(segments),
                (Quote::None, '<') => {
                    self.end_word(segments);
                    if self.chars.peek() == Some(&'<') {
                        self.chars.next();
                        if self.chars.peek() == Some(&'<') {
                            self.chars.next();
                        } else {
                            let dash = self.chars.peek() == Some(&'-');
                            if dash {
                                self.chars.next();
                            }
                            self.awaiting_delimiter = Some(dash);
                        }
                    }
                }
                (Quote::None, c) if c.is_whitespace() => self.end_word(segments),
                (Quote::None, '\'') => {
                    quote = Quote::Single;
                    self.in_word = true;
                }
                (Quote::None, '"') => {
                    quote = Quote::Double;
                    self.in_word = true;
                }
                (Quote::None, '$') if self.ansi_c && self.chars.peek() == Some(&'\'') => {
                    self.chars.next();
                    quote = Quote::AnsiC;
                    self.in_word = true;
                }
                (Quote::None, c) if c == self.escape => match self.chars.next() {
                    Some('\n') | None => {}
                    Some(next) => {
                        self.in_word = true;
                        self.word.push(next);
                    }
                },
                (Quote::Single | Quote::AnsiC, '\'') | (Quote::Double, '"') => quote = Quote::None,
                (Quote::Double, c) if c == self.escape => match self.chars.peek() {
                    Some('\n') => {
                        self.chars.next();
                    }
                    Some(&next) if matches!(next, '"' | '$' | '`') || next == self.escape => {
                        self.word.push(next);
                        self.chars.next();
                    }
                    _ => self.word.push(self.escape),
                },
                (Quote::AnsiC, '\\') => {
                    if let Some(next) = self.chars.next() {
                        self.word.push(ansi_c_escape(next));
                    }
                }
                (Quote::None | Quote::Single | Quote::Double | Quote::AnsiC, c) => {
                    self.in_word = true;
                    self.word.push(c);
                }
            }
            if unquoted {
                prev = c;
            }
        }
        self.end_word(segments);
    }

    /// Ends the word being read, if any: a heredoc's delimiter is noted, any other word kept.
    fn end_word(&mut self, segments: &mut Segments) {
        if !self.in_word {
            return;
        }
        self.in_word = false;
        let word = std::mem::take(&mut self.word);
        match self.awaiting_delimiter.take() {
            Some(dash) => self.heredocs.push((word, dash)),
            None => segments.push(word),
        }
    }

    /// Reads the bodies of the heredocs the line just ended opened, each into a segment that is
    /// not a command, its words split at whitespace.
    fn bodies(&mut self, segments: &mut Segments) {
        for (delimiter, dash) in std::mem::take(&mut self.heredocs) {
            segments.start(false);
            loop {
                let mut line = String::new();
                let mut ended = true;
                for c in self.chars.by_ref() {
                    if c == '\n' {
                        ended = false;
                        break;
                    }
                    line.push(c);
                }
                let text = if dash {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if text == delimiter {
                    break;
                }
                for word in line.split_whitespace() {
                    segments.push(word.to_owned());
                }
                if ended {
                    break;
                }
            }
        }
        segments.split();
    }
}

/// The character `\x` stands for in ANSI-C quoting (`$'…'`), for the basic escapes; any other
/// `x` stands for itself.
fn ansi_c_escape(c: char) -> char {
    match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        'a' => '\u{7}',
        'b' => '\u{8}',
        'e' | 'E' => '\u{1b}',
        'f' => '\u{c}',
        'v' => '\u{b}',
        other => other,
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
    use super::{Escape, Segments, candidates, shell_segments};

    fn segments(command: &str, limit: usize, escape: Escape) -> Vec<Vec<String>> {
        let mut segments = Segments::new(limit);
        shell_segments(command, escape, &mut segments);
        segments
            .iter()
            .map(|segment| {
                let mut words = segment.words.clone();
                if !segment.command {
                    words.insert(0, "(body)".to_owned());
                }
                words
            })
            .collect()
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
                vec!["echo", "a;b", "c && d", "x"],
                vec!["tee", "y"],
                vec!["z"],
                vec!["w", "2", "1"],
                vec!["v"],
                vec!["u"],
            ]
        );
        assert_eq!(
            segments("(cd D && x) ; { cd E; } &>log", 64, posix),
            [
                vec!["cd", "D"],
                vec!["x"],
                vec!["{", "cd", "E"],
                vec!["}", "log"]
            ]
        );
        assert_eq!(
            segments("a\\;b ; ;; c", 64, posix),
            [vec!["a;b"], vec!["c"]]
        );
        assert_eq!(segments("a; b; c", 2, posix), [vec!["a"], vec!["b"]]);
        assert_eq!(
            segments(
                "Set-Location D; Set-Content ..\\C.docx hi; & { sl E }",
                64,
                Escape::Backtick
            ),
            [
                vec!["Set-Location", "D"],
                vec!["Set-Content", r"..\C.docx", "hi"],
                vec!["{", "sl", "E", "}"]
            ]
        );
    }

    #[test]
    fn redirections_comments_continuations_and_ansi_c_quotes_are_read() {
        let posix = Escape::Backslash;
        assert_eq!(
            split(">Contract.docx&true", 64, posix),
            ["Contract.docx", "true"]
        );
        assert_eq!(
            segments("cat>Contract.docx<x", 64, posix),
            [vec!["cat", "Contract.docx", "x"]]
        );
        assert_eq!(
            segments("cd D # ; cd /tmp\necho a#b", 64, posix),
            [vec!["cd", "D"], vec!["echo", "a#b"]]
        );
        assert_eq!(
            segments("echo hi > Con\\\ntract.docx \"a\\\nb\"", 64, posix),
            [vec!["echo", "hi", "Contract.docx", "ab"]]
        );
        assert_eq!(
            split(r"echo $'Con\x74ract.docx' $'a\'b\n' x$'y'", 64, posix),
            ["echo", "Conx74ract.docx", "a'b\n", "xy"]
        );
        // PowerShell has no ANSI-C quotes, and its backtick joins lines.
        assert_eq!(
            split("Set-Content $'a' Con`\ntract.docx", 64, Escape::Backtick),
            ["Set-Content", "$a", "Contract.docx"]
        );
    }

    #[test]
    fn heredoc_bodies_are_not_commands() {
        let posix = Escape::Backslash;
        assert_eq!(
            segments(
                "cat <<'EOF' > x\ncd /tmp\nEOF\necho hi; cat <<-END\n\tcd /\n\tEND\nls <<<here",
                64,
                posix
            ),
            [
                vec!["cat", "x"],
                vec!["(body)", "cd", "/tmp"],
                vec!["echo", "hi"],
                vec!["cat"],
                vec!["(body)", "cd", "/"],
                vec!["ls", "here"],
            ]
        );
        // An unclosed heredoc runs to the end.
        assert_eq!(
            segments("cat <<E\ncd x", 64, posix),
            [vec!["cat"], vec!["(body)", "cd", "x"]]
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
