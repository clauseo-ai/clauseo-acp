//! A POSIX shell command split into simple commands, for [`super::add_input`]: words with
//! their quotes taken off, `>` and `>>` targets apart, the separators kept, heredoc bodies
//! skipped.

use std::iter::Peekable;
use std::str::Chars;

use super::{Segment, Sep, Word};

/// The words of the segment being read, and where the word being read goes.
#[derive(Default)]
pub(super) struct Reader {
    pub(super) out: Vec<Segment>,
    pub(super) segment: Segment,
    pub(super) word: String,
    /// A word has begun (an empty quoted word counts).
    pub(super) started: bool,
    /// It holds no expansion, glob or `~` so far.
    pub(super) unclear: bool,
    /// It is a redirect's target.
    pub(super) redirect: bool,
    /// How many parentheses are open.
    pub(super) depth: usize,
}

impl Reader {
    pub(super) fn push(&mut self, c: char) {
        self.started = true;
        self.word.push(c);
    }

    pub(super) fn end_word(&mut self) {
        if self.started {
            let word = Word {
                text: std::mem::take(&mut self.word),
                clear: !self.unclear,
            };
            if self.redirect {
                self.segment.redirects.push(word);
            } else {
                self.segment.words.push(word);
            }
            self.redirect = false;
        }
        self.started = false;
        self.unclear = false;
    }

    /// Ends the segment being read at `sep` (`Sep::End` at the command's end).
    pub(super) fn end_segment(&mut self, sep: Sep) {
        self.end_word();
        self.redirect = false;
        let mut segment = std::mem::take(&mut self.segment);
        if !segment.words.is_empty() || !segment.redirects.is_empty() {
            segment.ender = sep;
            segment.nested = self.depth > 0;
            self.out.push(segment);
        }
        match sep {
            Sep::Open => self.depth += 1,
            Sep::Close => self.depth = self.depth.saturating_sub(1),
            Sep::Then | Sep::And | Sep::Or | Sep::Pipe | Sep::Background | Sep::End => {}
        }
    }
}

/// The simple commands of `script`, as a POSIX shell splits it.
pub(super) fn segments(script: &str) -> Vec<Segment> {
    let mut r = Reader::default();
    let mut heredocs: Vec<Heredoc> = Vec::new();
    let mut chars = script.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                r.started = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    r.word.push(q);
                }
            }
            '"' => {
                r.started = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' if matches!(chars.peek(), Some('"' | '\\' | '$' | '`')) => {
                            r.word.extend(chars.next());
                        }
                        '$' | '`' => {
                            r.unclear = true;
                            r.word.push(q);
                        }
                        _ => r.word.push(q),
                    }
                }
            }
            '\\' => {
                r.started = true;
                r.word.extend(chars.next());
            }
            '$' | '`' | '*' | '?' | '[' | '{' => {
                r.unclear = true;
                r.push(c);
            }
            '~' if !r.started => {
                r.unclear = true;
                r.push(c);
            }
            '>' => {
                // `2>`: the number before it is no word
                if r.started && !r.unclear && r.word.chars().all(|d| d.is_ascii_digit()) {
                    r.word.clear();
                    r.started = false;
                }
                r.end_word();
                if chars.peek() == Some(&'>') {
                    chars.next();
                }
                if chars.peek() == Some(&'&') {
                    // `>&2`: a copy of another output, no file
                    chars.next();
                    while chars.next_if(|d| d.is_ascii_digit() || *d == '-').is_some() {}
                } else {
                    r.redirect = true;
                }
            }
            '<' => {
                // the input's file is read, not written: its word is dropped
                r.end_word();
                let mut extra = 0;
                while chars.next_if_eq(&'<').is_some() {
                    extra += 1;
                }
                if extra == 1 {
                    // `<<WORD` or `<<-WORD`: the lines after this one are its body, not commands
                    let strip_tabs = chars.next_if_eq(&'-').is_some();
                    while chars.next_if(|d| *d == ' ' || *d == '\t').is_some() {}
                    let word = heredoc_word(&mut chars);
                    heredocs.push(Heredoc { word, strip_tabs });
                } else {
                    while chars.next_if(|d| *d == ' ' || *d == '<').is_some() {}
                    while chars
                        .next_if(|d| !d.is_whitespace() && !";&|<>".contains(*d))
                        .is_some()
                    {}
                }
            }
            ';' | '&' | '|' | '\n' | '(' | ')' => {
                let double = matches!(c, '&' | '|') && chars.next_if_eq(&c).is_some();
                r.end_segment(sep_of(c, double));
                if c == '\n' {
                    skip_heredoc_bodies(&mut chars, &mut heredocs);
                }
            }
            c if c.is_whitespace() => r.end_word(),
            _ => r.push(c),
        }
    }
    r.end_segment(Sep::End);
    r.out
}

/// The separator `c` (doubled: `&&`, `||`).
pub(super) fn sep_of(c: char, double: bool) -> Sep {
    match (c, double) {
        ('&', true) => Sep::And,
        ('&', false) => Sep::Background,
        ('|', true) => Sep::Or,
        ('|', false) => Sep::Pipe,
        ('(', _) => Sep::Open,
        (')', _) => Sep::Close,
        _ => Sep::Then,
    }
}

/// A heredoc opened on a line: its delimiter word, and whether `<<-` strips leading tabs.
struct Heredoc {
    pub(super) word: String,
    strip_tabs: bool,
}

/// A heredoc's delimiter, read up to a blank or an operator with its quotes taken off
/// (`'EOF'`, `"EOF"` and `EOF` are all `EOF`).
fn heredoc_word(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut word = String::new();
    while let Some(c) = chars.next_if(|d| !d.is_whitespace() && !";&|<>()".contains(*d)) {
        match c {
            '\'' => word.extend(chars.by_ref().take_while(|q| *q != '\'')),
            '"' => word.extend(chars.by_ref().take_while(|q| *q != '"')),
            '\\' => word.extend(chars.next()),
            _ => word.push(c),
        }
    }
    word
}

/// Skips the bodies of the heredocs opened on the line just ended, in order. Each runs up to
/// the line that is its delimiter (leading tabs taken off first for `<<-`), or to the end.
fn skip_heredoc_bodies(chars: &mut Peekable<Chars<'_>>, heredocs: &mut Vec<Heredoc>) {
    for doc in heredocs.drain(..) {
        loop {
            let line: String = chars.by_ref().take_while(|c| *c != '\n').collect();
            let content = if doc.strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line.as_str()
            };
            if content == doc.word || chars.peek().is_none() {
                break;
            }
        }
    }
}
