//! PowerShell's own reading of a word, for [`super::shell_segments`] under
//! [`super::Escape::Backtick`], after `about_Quoting_Rules` and `about_Special_Characters`
//! (learn.microsoft.com/powershell/module/microsoft.powershell.core/about/) and the tokenizer's
//! quote sets (`System.Management.Automation.Language.CharExtensions.IsSingleQuote` and
//! `IsDoubleQuote`, over `SpecialChars` in `tokenizer.cs` of the PowerShell repository):
//!
//! - Single quotes: `'` and the typographic `‘` `’` `‚` `‛` (U+2018, U+2019, U+201A, U+201B),
//!   any of them opening or closing. Inside, two of them in a row stand for the second; nothing
//!   else is special.
//! - Double quotes: `"` and `“` `”` `„` (U+201C, U+201D, U+201E). Inside, two in a row stand for
//!   the second, a backtick escapes the next character, and `$` starts an expansion.
//! - Outside quotes a backtick escapes the next character too; `` `n ``, `` `t `` and the other
//!   escape sequences stand for their control characters, in quotes and out; a backtick before a
//!   new line joins the lines.
//! - `$name`, `${…}`, `$(…)`, `$_`, `$?`, `$^`, `$$` and `$env:X`: a value only the run knows.
//! - `@'` or `@"` at the end of a line opens a here-string, to a line starting `'@` or `"@`.
//! - An unquoted `,` makes an array: each element is a value of its own.

use std::iter::Peekable;
use std::str::Chars;

/// Whether PowerShell's tokenizer takes `c` as a single quote.
pub(super) fn is_single(c: char) -> bool {
    matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}')
}

/// Whether PowerShell's tokenizer takes `c` as a double quote.
pub(super) fn is_double(c: char) -> bool {
    matches!(c, '"' | '\u{201c}' | '\u{201d}' | '\u{201e}')
}

/// The character a backtick before `c` stands for (`about_Special_Characters`), reading a
/// `` `u{XXXX} `` escape's digits from `chars`. `` `e `` and `` `u{…} `` are PowerShell 7's:
/// Windows PowerShell 5.1 reads `"`u{43}"` as `u{43}` and 7.4 as `C` (VM row 9). Decoding always
/// reads as 7 does, a name 5.1 would not write; that fails safe (the name backstop and the held
/// file's path still match what 7 writes), so it stays.
pub(super) fn escaped(c: char, chars: &mut Peekable<Chars<'_>>) -> char {
    match c {
        '0' => '\0',
        'a' => '\u{7}',
        'b' => '\u{8}',
        'e' => '\u{1b}',
        'f' => '\u{c}',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'v' => '\u{b}',
        'u' if chars.peek() == Some(&'{') => {
            let mut ahead = chars.clone();
            ahead.next();
            let digits: String = ahead.by_ref().take_while(|d| *d != '}').take(7).collect();
            match u32::from_str_radix(&digits, 16)
                .ok()
                .and_then(char::from_u32)
            {
                Some(decoded) if (1..=6).contains(&digits.len()) => {
                    *chars = ahead;
                    decoded
                }
                _ => 'u',
            }
        }
        other => other,
    }
}

/// Whether a `$` followed by `next` starts an expansion: a variable's name (letters, digits,
/// `_`, a scope or drive `:`), `{`, `(`, or one of the automatic `$_`, `$?`, `$^`, `$$`.
pub(super) fn expands(next: Option<char>) -> bool {
    next.is_some_and(|c| {
        c.is_alphanumeric() || matches!(c, '_' | '{' | '(' | '?' | '^' | '$' | ':')
    })
}

/// Reads an expansion's text after its `$` (whose next character [`expands`]) into `word`: a
/// `(…)` or `{…}` to its matching close, quotes inside it skipped; else the name's characters.
/// `false` when a `(` or `{` never closes.
pub(super) fn expansion(chars: &mut Peekable<Chars<'_>>, word: &mut String) -> bool {
    match chars.peek().copied() {
        Some(open @ ('(' | '{')) => {
            let close = if open == '(' { ')' } else { '}' };
            let mut depth = 0usize;
            let mut quote: Option<bool> = None;
            while let Some(c) = chars.next() {
                word.push(c);
                match (quote, c) {
                    (_, '`') => word.extend(chars.next()),
                    (None, c) if is_single(c) => quote = Some(true),
                    (None, c) if is_double(c) => quote = Some(false),
                    (Some(true), c) if is_single(c) => quote = None,
                    (Some(false), c) if is_double(c) => quote = None,
                    (None, c) if c == open => depth += 1,
                    (None, c) if c == close => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
            false
        }
        Some(first)
            if matches!(first, '_' | '?' | '^' | '$')
                && !chars.clone().nth(1).is_some_and(char::is_alphanumeric) =>
        {
            word.extend(chars.next());
            true
        }
        _ => {
            while let Some(c) = chars.next_if(|c| c.is_alphanumeric() || matches!(c, '_' | ':')) {
                word.push(c);
            }
            true
        }
    }
}

/// Whether `text` (a double-quoted here-string's body) holds an expansion: a `$` that
/// [`expands`], not escaped by a backtick.
pub(super) fn has_expansion(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' => {
                chars.next();
            }
            '$' if expands(chars.peek().copied()) => return true,
            _ => {}
        }
    }
    false
}

/// When `chars` (just after an `@`) open a here-string (a quote, then only spaces or tabs to the
/// end of the line), reads it: its body, the lines up to one starting with a quote of the same
/// kind and `@`, and whether it is a double-quoted one. `Err` with the body read when it never
/// closes; `None`, nothing read, when this `@` opens none.
pub(super) fn here_string(
    chars: &mut Peekable<Chars<'_>>,
) -> Option<Result<(String, bool), String>> {
    let mut ahead = chars.clone();
    let quote = ahead.next().filter(|c| is_single(*c) || is_double(*c))?;
    let same = |c: char| {
        if is_single(quote) {
            is_single(c)
        } else {
            is_double(c)
        }
    };
    while ahead.next_if(|c| matches!(c, ' ' | '\t')).is_some() {}
    // The line ends at CR, LF or CRLF, as everywhere in PowerShell.
    let cr = ahead.next_if_eq(&'\r').is_some();
    if ahead.next_if_eq(&'\n').is_none() && !cr {
        return None;
    }
    *chars = ahead;
    let mut body = String::new();
    let mut line_start = true;
    while let Some(c) = chars.next() {
        if line_start && same(c) && chars.peek() == Some(&'@') {
            chars.next();
            if body.ends_with('\n') {
                body.pop();
            }
            if body.ends_with('\r') {
                body.pop();
            }
            return Some(Ok((body, !is_single(quote))));
        }
        line_start = matches!(c, '\n' | '\r');
        body.push(c);
    }
    Some(Err(body))
}
