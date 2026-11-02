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
/// commands, but their words still name files), and the targets of its output redirections
/// (`>x`, `>>x`, `2>x`, `&>x`; a duplication such as `2>&1` as `&1`), which are words too.
#[derive(Debug, Default)]
pub(crate) struct Segment {
    pub(crate) words: Vec<String>,
    pub(crate) command: bool,
    pub(crate) redirects: Vec<String>,
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
                redirects: Vec::new(),
            }),
        }
    }

    /// Notes `target` as an output redirection of the last segment.
    fn redirect(&mut self, target: String) {
        if let Some(last) = self.list.last_mut() {
            last.redirects.push(target);
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
///   quoted text with bash's escapes ([`ansi_c_char`]). An unclosed quote ends with the command.
/// - An unquoted `#` at a word's start begins a comment, to the end of the line; under
///   [`Escape::Backtick`], `<#` begins a block comment, to `#>`.
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
        redirecting: None,
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
    /// After `>`: the next word is a redirection's target; whether it duplicates a descriptor.
    redirecting: Option<bool>,
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
                    if prev == '>' {
                        self.redirecting = Some(true);
                    }
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
                (Quote::None, '>') => {
                    self.end_word(segments);
                    self.redirecting = Some(false);
                }
                (Quote::None, '<') if self.escape == '`' && self.chars.peek() == Some(&'#') => {
                    self.end_word(segments);
                    let mut last = ' ';
                    for next in self.chars.by_ref() {
                        if last == '#' && next == '>' {
                            break;
                        }
                        last = next;
                    }
                }
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
                    if let Some(decoded) = ansi_c_char(&mut self.chars) {
                        self.word.push(decoded);
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
        if let Some(dash) = self.awaiting_delimiter.take() {
            self.heredocs.push((word, dash));
            return;
        }
        if let Some(duplicate) = self.redirecting.take() {
            segments.redirect(if duplicate {
                format!("&{word}")
            } else {
                word.clone()
            });
        }
        segments.push(word);
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

/// The character an ANSI-C escape (`$'…'`) stands for, read from `chars` just after its
/// backslash, as bash reads it: `\n`, `\t`, `\r`, `\a`, `\b`, `\e`, `\f`, `\v`, `\xHH` (one or two
/// hex digits), `\NNN` (one to three octal digits), `\uHHHH` and `\UHHHHHHHH` (up to four or eight
/// hex digits); any other character stands for itself. `None` at the end of the text.
pub(crate) fn ansi_c_char(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<char> {
    let c = chars.next()?;
    let mut number = |radix: u32, most: usize, first: Option<char>| {
        let mut value: u32 = first.and_then(|d| d.to_digit(radix)).unwrap_or(0);
        let mut count = usize::from(first.is_some());
        while count < most {
            let Some(digit) = chars.peek().and_then(|d| d.to_digit(radix)) else {
                break;
            };
            value = value.saturating_mul(radix).saturating_add(digit);
            chars.next();
            count += 1;
        }
        (count > 0).then(|| char::from_u32(value).unwrap_or('\u{fffd}'))
    };
    Some(match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        'a' => '\u{7}',
        'b' => '\u{8}',
        'e' | 'E' => '\u{1b}',
        'f' => '\u{c}',
        'v' => '\u{b}',
        'x' => number(16, 2, None).unwrap_or('x'),
        'u' => number(16, 4, None).unwrap_or('u'),
        'U' => number(16, 8, None).unwrap_or('U'),
        '0'..='7' => number(8, 3, Some(c)).unwrap_or(c),
        other => other,
    })
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
mod tests;
