//! The one command shape the stetdoc read fence lets through, read without the shell's help: a
//! short list of simple commands joined by `;`, `&&`, `||`, `|` or a new line, each word a plain
//! literal (a bare word of safe characters, a single-quoted string, or a double-quoted string
//! with nothing the shell would change), with no wildcard anywhere and no redirection but
//! `2>&1`. Anything else is not that shape ([`plain_segments`] gives `None`), and a command
//! that mentions stetdoc ([`mentions_stetdoc`]) in another shape is refused. Nothing here tries
//! to read what an unusual line would do; it only says whether the line is the plain shape.

/// The most bytes, words and simple commands a line of the plain shape has.
const MAX_BYTES: usize = 16 * 1024;
const MAX_WORDS: usize = 256;
const MAX_SEGMENTS: usize = 32;

/// The only redirection the shape takes: standard error into standard output.
const ERR_TO_OUT: &str = "2>&1";

/// One simple command of a plain line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Segment {
    /// Its words, quotes off.
    pub(super) words: Vec<String>,
    /// Whether a pipe feeds it (`a | b`: `b`).
    pub(super) piped_in: bool,
}

/// Whether `text` mentions stetdoc in any case, with the quotes and escape characters that
/// could split the name taken out first (`s't'etdoc`, `st\etdoc`, ``st`etdoc``, `st^etdoc`).
pub(super) fn mentions_stetdoc(text: &str) -> bool {
    let joined: String = text
        .chars()
        .filter(|c| !matches!(c, '\'' | '"' | '\\' | '`' | '^'))
        .flat_map(char::to_lowercase)
        .collect();
    joined.contains("stetdoc")
}

/// The simple commands of `text` when it is the plain shape for its shell (`powershell`, else
/// Bash), else `None`.
pub(super) fn plain_segments(text: &str, powershell: bool) -> Option<Vec<Segment>> {
    if text.len() > MAX_BYTES || text.chars().any(is_odd_quote) {
        return None;
    }
    let mut line = Line::default();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => line.end_word()?,
            ';' | '\n' => line.end_segment(false)?,
            '&' if chars.next_if_eq(&'&').is_some() => line.end_segment(false)?,
            '|' if chars.next_if_eq(&'|').is_some() => line.end_segment(false)?,
            '|' if chars.peek() != Some(&'&') => line.end_segment(true)?,
            '\'' | '"' => {
                if line.started {
                    // A quote inside a word joins two forms in one word: not plain.
                    return None;
                }
                let mut quoted = String::new();
                loop {
                    let next = chars.next()?;
                    if next == c {
                        break;
                    }
                    if c == '"' && !plain_in_double(next, powershell) {
                        return None;
                    }
                    quoted.push(next);
                }
                // The word ends here: what follows must end it.
                if chars
                    .peek()
                    .is_some_and(|next| !matches!(next, ' ' | '\t' | ';' | '\n' | '&' | '|'))
                {
                    return None;
                }
                line.word.push_str(&quoted);
                line.started = true;
                line.quoted = true;
            }
            '2' if !line.started && chars.clone().take(3).eq(ERR_TO_OUT.chars().skip(1)) => {
                let after = chars.clone().nth(3);
                if line.words.is_empty()
                    || after.is_some_and(|next| !matches!(next, ' ' | '\t' | ';' | '\n' | '|'))
                {
                    return None;
                }
                chars.by_ref().take(3).for_each(drop);
                line.skipped_redirect = true;
            }
            c if plain_bare(c, powershell) => {
                if c == '~'
                    && !line
                        .word
                        .chars()
                        .next_back()
                        .is_some_and(char::is_alphanumeric)
                {
                    // `~` starts a home folder at a word's start, or after `=` or `:` in Bash.
                    return None;
                }
                if line.quoted {
                    return None;
                }
                line.word.push(c);
                line.started = true;
            }
            _ => return None,
        }
    }
    line.end_segment(false)?;
    if line.pipe_pending {
        return None;
    }
    Some(line.segments)
}

/// A quote mark PowerShell takes as `'` or `"` and Bash as a letter: never plain.
fn is_odd_quote(c: char) -> bool {
    matches!(c, '\u{2018}'..='\u{201F}')
}

/// Whether `c` may stand in a bare word: letters and digits in any script, and punctuation that
/// neither shell gives a meaning in a word. PowerShell takes `\` as a plain character (a Windows
/// path's separator); Bash an escape, so not plain there. `,` makes an array in PowerShell.
fn plain_bare(c: char, powershell: bool) -> bool {
    c.is_alphanumeric()
        || matches!(c, '-' | '_' | '.' | '/' | ':' | '+' | '=' | '%' | '~')
        || (powershell && c == '\\')
        || (!powershell && matches!(c, ',' | '@'))
}

/// Whether `c` stays as it is inside double quotes: no `$` or backtick in either shell, and no
/// `\` or `!` in Bash.
fn plain_in_double(c: char, powershell: bool) -> bool {
    !matches!(c, '$' | '`') && (powershell || !matches!(c, '\\' | '!'))
}

/// The line read so far.
#[derive(Default)]
struct Line {
    segments: Vec<Segment>,
    words: Vec<String>,
    word: String,
    /// Whether the current word has begun (an empty quoted word counts).
    started: bool,
    /// Whether the current word was quoted: nothing may follow in the same word.
    quoted: bool,
    /// Whether the segment being read is fed by a pipe.
    piped_in: bool,
    /// Whether the last separator was a pipe, so a segment must follow.
    pipe_pending: bool,
    /// Whether `2>&1` stood in the segment being read.
    skipped_redirect: bool,
    count: usize,
}

impl Line {
    /// The current word, ended; `None` past [`MAX_WORDS`] or for a wildcard in it.
    fn end_word(&mut self) -> Option<()> {
        if !self.started {
            return Some(());
        }
        let word = std::mem::take(&mut self.word);
        if word.contains(['*', '?', '[']) || word == "--%" {
            return None;
        }
        self.count += 1;
        if self.count > MAX_WORDS {
            return None;
        }
        self.words.push(word);
        self.started = false;
        self.quoted = false;
        Some(())
    }

    /// The current segment, ended by a separator (`pipe` when it is `|`).
    fn end_segment(&mut self, pipe: bool) -> Option<()> {
        self.end_word()?;
        if self.words.is_empty() {
            // `;;`, a leading `;` or an empty line ends nothing; a pipe at either end is no shape.
            if pipe || self.pipe_pending || self.skipped_redirect {
                return None;
            }
            return Some(());
        }
        self.segments.push(Segment {
            words: std::mem::take(&mut self.words),
            piped_in: self.piped_in,
        });
        if self.segments.len() > MAX_SEGMENTS {
            return None;
        }
        self.piped_in = pipe;
        self.pipe_pending = pipe;
        self.skipped_redirect = false;
        Some(())
    }
}

#[cfg(test)]
#[path = "shape_tests.rs"]
mod tests;
