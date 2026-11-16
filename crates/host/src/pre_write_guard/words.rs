//! The shell side of the guard's heuristic: a command's words, the simple shell way or
//! `PowerShell`'s (`powershell.rs`), and the file names a word may hold. Every reader of a
//! `PowerShell` command in the host reads it here.

mod powershell;
mod segments;

use segments::Known;
pub(crate) use segments::{Joint, Role, Segment, Segments};

/// Which character escapes the next one in a shell command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Escape {
    /// POSIX shells (`Bash`): a backslash.
    Backslash,
    /// `PowerShell`: a backtick; a backslash is a plain character (a Windows path's separator).
    Backtick,
}

/// What an unquoted `,` becomes in a word under `PowerShell`'s rules, for every reader:
/// `PowerShell`'s array separator (and cmd's argument delimiter), told apart from a quoted comma,
/// which is part of a name (`'C:\Clients\Smith, Jones & Co'`). [`candidates`] takes each
/// element of such a word as a name of its own.
pub(crate) const ARRAY_COMMA: char = '\u{1f}';

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
///
/// Under [`Escape::Backtick`] a word is read as `PowerShell` reads it (`powershell.rs`, after
/// `about_Quoting_Rules`): `''` in single quotes and `""` in double quotes stand for one quote,
/// the typographic quotes count as quotes, a backtick escapes in double quotes and out of quotes
/// (`` `n `` a new line), a here-string (`@'` … `'@`) is one word, an unquoted `,` becomes
/// [`ARRAY_COMMA`], and a word with an expansion (`$name`, `$(…)`, in double quotes or bare) is
/// unknown ([`Segment::unknown`]); the commands inside a `$(…)` are read too, as segments of
/// their own after the command's. Under either escape, a quote left open makes the reading
/// [`Segments::unsure`] and its word unknown.
pub(crate) fn shell_segments(command: &str, escape: Escape, segments: &mut Segments) {
    let escape = match escape {
        Escape::Backslash => '\\',
        Escape::Backtick => '`',
    };
    let mut scanner = Scanner {
        chars: command.chars().peekable(),
        escape,
        ansi_c: escape == '\\',
        powershell: escape == '`',
        word: String::new(),
        in_word: false,
        quoted: false,
        unknown: false,
        stream: false,
        awaiting_delimiter: None,
        redirecting: None,
        heredocs: Vec::new(),
        inner: Vec::new(),
    };
    segments.split();
    scanner.run(segments);
    let inner = std::mem::take(&mut scanner.inner);
    for body in inner {
        shell_segments(&body, Escape::Backtick, segments);
    }
}

/// The state of [`shell_segments`].
struct Scanner<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    escape: char,
    /// Whether `$'…'` is ANSI-C quoting (POSIX shells).
    ansi_c: bool,
    word: String,
    in_word: bool,
    /// Whether a quote or the escape character was part of the word being read.
    quoted: bool,
    /// `PowerShell`'s rules (`powershell.rs`).
    powershell: bool,
    /// Whether the word being read holds an expansion, or a quote that never closed.
    unknown: bool,
    /// Whether the word being read is a stream's number before `>` ([`Role::Stream`]).
    stream: bool,
    /// After `<<` or `<<-`: the next word is a heredoc's delimiter; whether tabs are stripped.
    awaiting_delimiter: Option<bool>,
    /// After `>`: the next word is a redirection's target; whether it duplicates a descriptor.
    redirecting: Option<bool>,
    /// Heredocs whose bodies start after this line: delimiter, tabs stripped.
    heredocs: Vec<(String, bool)>,
    /// The bodies of the `$(…)` read so far (`PowerShell`), read as commands after this one.
    inner: Vec<String>,
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
            if self.powershell
                && let Some(next) = self.powershell_char(quote, c, segments)
            {
                quote = next;
                if unquoted {
                    prev = c;
                }
                continue;
            }
            match (quote, c) {
                (Quote::None, '#') if !self.in_word => {
                    // `PowerShell` ends a comment at a CR too (VM row 2, 5.1 and 7.4).
                    let cr_ends = self.powershell;
                    while self
                        .chars
                        .peek()
                        .is_some_and(|&next| next != '\n' && !(cr_ends && next == '\r'))
                    {
                        self.chars.next();
                    }
                }
                // `PowerShell` takes a lone CR, and CRLF, as one new line (VM rows 2 and 3).
                (Quote::None, '\r') if self.powershell => {
                    self.chars.next_if_eq(&'\n');
                    self.operator('\n', prev, segments);
                }
                (Quote::None, '\n' | ';' | '(' | ')' | '&' | '|' | '>') => {
                    self.operator(c, prev, segments);
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
                (Quote::None, '\'') => quote = self.open(Quote::Single),
                (Quote::None, '"') => quote = self.open(Quote::Double),
                (Quote::None, '$') if self.ansi_c && self.chars.peek() == Some(&'\'') => {
                    self.chars.next();
                    quote = self.open(Quote::AnsiC);
                }
                (Quote::None, c) if c == self.escape => match self.chars.next() {
                    Some('\n') | None => {}
                    Some(next) => {
                        self.in_word = true;
                        self.quoted = true;
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
        if quote != Quote::None {
            self.unknown = true;
            segments.set_unsure();
        }
        self.end_word(segments);
        if self.redirecting.is_some() {
            segments.set_unsure();
        }
    }

    /// An unquoted new line, `;`, `(`, `)`, `&`, `|` or `>` (`c`, after the unquoted `prev`):
    /// the word before it ends, and the segment too at each [`Joint`]; after `>`, the next word
    /// is a redirection's target, and the word it ends a stream's number when it is one.
    fn operator(&mut self, c: char, prev: char, segments: &mut Segments) {
        if c == '>'
            && self.in_word
            && !self.quoted
            && (self.word == "*" || self.word.chars().all(|d| d.is_ascii_digit()))
        {
            self.stream = true;
        }
        self.end_word(segments);
        let next = self.chars.peek().copied();
        match c {
            '\n' => {
                segments.end(Joint::Then);
                self.bodies(segments);
            }
            ';' => segments.end(Joint::Then),
            '(' => {
                // `> (Join-Path …)` writes where the expression leads (VM row 5): a target only
                // the run knows.
                if self.redirecting.take().is_some() {
                    segments.redirect("(".to_owned());
                    let known = Known {
                        quoted: false,
                        unknown: true,
                        role: Role::Target,
                    };
                    segments.push_word("(".to_owned(), known);
                }
                segments.end(Joint::Open);
            }
            ')' => segments.end(Joint::Close),
            '&' => {
                if prev == '>' {
                    self.redirecting = Some(true);
                }
                if next == Some('&') {
                    self.chars.next();
                    segments.end(Joint::And);
                } else if !matches!(prev, '>' | '<') && next != Some('>') {
                    segments.end(Joint::Background);
                }
            }
            '|' if prev != '>' => {
                let joint = if next == Some('|') {
                    Joint::Or
                } else {
                    Joint::Pipe
                };
                if matches!(next, Some('|' | '&')) {
                    self.chars.next();
                }
                segments.end(joint);
            }
            '>' => self.redirecting = Some(false),
            _ => {}
        }
    }

    /// One character under `PowerShell`'s rules (`powershell.rs`), where they differ from the
    /// simple shell way: the quote after it, or `None` when the simple way reads it.
    fn powershell_char(&mut self, quote: Quote, c: char, segments: &mut Segments) -> Option<Quote> {
        use powershell::{escaped, expands, is_double, is_single};
        match quote {
            Quote::None if is_single(c) => Some(self.open(Quote::Single)),
            Quote::None if is_double(c) => Some(self.open(Quote::Double)),
            Quote::None | Quote::Double if c == '`' => {
                match self.chars.next() {
                    // A backtick right before CR, LF or CRLF joins the lines; before anything
                    // else, a blank too, it escapes that character (VM rows 1 and 1b).
                    Some('\n') | None => {}
                    Some('\r') => {
                        self.chars.next_if_eq(&'\n');
                    }
                    Some(next) => {
                        self.in_word = true;
                        self.quoted = true;
                        let decoded = escaped(next, &mut self.chars);
                        self.word.push(decoded);
                    }
                }
                Some(quote)
            }
            Quote::None | Quote::Double if c == '$' && expands(self.chars.peek().copied()) => {
                self.in_word = true;
                self.unknown = true;
                self.word.push(c);
                let start = self.word.len();
                let closed = powershell::expansion(&mut self.chars, &mut self.word);
                if !closed {
                    segments.set_unsure();
                }
                if let Some(body) = self.word.get(start..).and_then(|e| e.strip_prefix('(')) {
                    let body = if closed {
                        body.strip_suffix(')').unwrap_or(body)
                    } else {
                        body
                    };
                    self.inner.push(body.to_owned());
                }
                Some(quote)
            }
            Quote::None if c == ',' => {
                self.in_word = true;
                self.word.push(ARRAY_COMMA);
                Some(quote)
            }
            // Blanks after an array's comma continue the array (`'a', $p`): one word still.
            Quote::None if c.is_whitespace() && self.word.ends_with(ARRAY_COMMA) => Some(quote),
            Quote::None if c == '@' => {
                let (body, double) = match powershell::here_string(&mut self.chars)? {
                    Ok(read) => read,
                    Err(body) => {
                        segments.set_unsure();
                        self.unknown = true;
                        (body, false)
                    }
                };
                self.unknown |= double && powershell::has_expansion(&body);
                self.in_word = true;
                self.quoted = true;
                self.word.push_str(&body);
                Some(quote)
            }
            Quote::Single if is_single(c) => Some(self.close_or_double(quote, is_single, segments)),
            Quote::Double if is_double(c) => Some(self.close_or_double(quote, is_double, segments)),
            Quote::None | Quote::Single | Quote::Double | Quote::AnsiC => None,
        }
    }

    /// At a quote of `quote`'s kind (`same`): two in a row stand for the second, kept in the word
    /// (`quote` stays open; `'a'’b'` keeps `’`, VM row 8); one alone closes it. Right after the
    /// closing quote, an unquoted `.`, `[` or `(` makes the word unknown (`'abc'.ToUpper()` calls
    /// the method, VM row 4a), and bare text starts another word (`'a'b` is two arguments, VM
    /// row 4b): rows observed on Windows PowerShell 5.1 and PowerShell 7.4, 16 Nov 2026.
    fn close_or_double(
        &mut self,
        quote: Quote,
        same: fn(char) -> bool,
        segments: &mut Segments,
    ) -> Quote {
        use powershell::{is_double, is_single};
        if let Some(second) = self.chars.next_if(|next| same(*next)) {
            self.word.push(second);
            return quote;
        }
        match self.chars.peek().copied() {
            Some('.' | '[' | '(') => self.unknown = true,
            Some(next)
                if !next.is_whitespace()
                    && !is_single(next)
                    && !is_double(next)
                    && !matches!(next, ',' | ';' | '|' | '&' | ')' | '>' | '<' | '}') =>
            {
                self.end_word(segments);
            }
            _ => {}
        }
        Quote::None
    }

    /// Opens `quote` in the word being read, which so counts as quoted.
    fn open(&mut self, quote: Quote) -> Quote {
        self.in_word = true;
        self.quoted = true;
        quote
    }

    /// Ends the word being read, if any: a heredoc's delimiter is noted, any other word kept.
    fn end_word(&mut self, segments: &mut Segments) {
        if !self.in_word {
            return;
        }
        self.in_word = false;
        let quoted = std::mem::take(&mut self.quoted);
        let unknown = std::mem::take(&mut self.unknown);
        let stream = std::mem::take(&mut self.stream);
        let word = std::mem::take(&mut self.word);
        if let Some(dash) = self.awaiting_delimiter.take() {
            self.heredocs.push((word, dash));
            return;
        }
        let mut role = if stream { Role::Stream } else { Role::Arg };
        if let Some(duplicate) = self.redirecting.take() {
            role = Role::Target;
            let target = if duplicate {
                format!("&{word}")
            } else {
                word.clone()
            };
            segments.redirect(target);
        }
        let known = Known {
            quoted,
            unknown,
            role,
        };
        segments.push_word(word, known);
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

/// A `PowerShell` command's simple commands, each as its words, as [`shell_segments`] reads them
/// under [`Escape::Backtick`] with no limit: an array's elements (`'a','b'`) each a word of their
/// own, a quoted comma part of its word; and a word of more than one line (a here-string's body)
/// also split at blanks, so a path written inside it is a word too, as the data folder fence's
/// own reader read it before. For readers that keep no other state per word (the data folder
/// fence's, `agent/data_fence/command.rs`).
pub(crate) fn powershell_words(command: &str) -> Vec<Vec<String>> {
    let mut segments = Segments::new(usize::MAX);
    shell_segments(command, Escape::Backtick, &mut segments);
    segments
        .iter()
        .map(|segment| {
            segment
                .words
                .iter()
                .flat_map(|word| word.split(ARRAY_COMMA))
                .flat_map(word_and_lines)
                .filter(|word| !word.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|words| !words.is_empty())
        .collect()
}

/// `word`, then, when it has more than one line, its parts between blanks, quotes trimmed
/// ([`powershell_words`]).
fn word_and_lines(word: &str) -> impl Iterator<Item = &str> {
    let parts = word.contains(['\n', '\r']).then(|| {
        word.split_whitespace()
            .map(|part| part.trim_matches(['\'', '"']))
    });
    std::iter::once(word).chain(parts.into_iter().flatten())
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
/// A word holding [`ARRAY_COMMA`] gives those of each element (`-Path 'a','Contract.docx'`).
pub(crate) fn candidates(word: &str) -> impl Iterator<Item = &str> {
    word.split(ARRAY_COMMA).flat_map(element_candidates)
}

/// [`candidates`] of one element of a word.
fn element_candidates(word: &str) -> impl Iterator<Item = &str> {
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
#[cfg(test)]
mod tests_powershell;
#[cfg(test)]
mod tests_rows;
