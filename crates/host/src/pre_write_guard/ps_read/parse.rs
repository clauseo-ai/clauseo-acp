//! The PowerShell lines the read allowlist understands, and the command it writes in their
//! place. Pure text: no file system, so every case is tested on every OS.
//!
//! The grammar (anything else is `None`, and the hook says nothing):
//!
//! ```text
//! line   := ws? stmt (ws? ';' ws? stmt)* ws?           at most 4096 bytes and 4 statements
//! stmt   := source (ws? '|' ws? select)?
//! source := list | read
//! list   := (Get-ChildItem | gci | ls | dir) [PATH] [-Recurse] [-File | -Directory]
//!           [-Force] [-Name]                            PATH: positional, -Path or -LiteralPath
//! read   := (Get-Content | gc | cat | type) PATH [-TotalCount N | -Tail N | -Raw]
//!           (-TotalCount also spelled -Head or -First, -Tail also -Last)
//! select := (Select-Object | select) [PROPS] [-First N]  at least one; PROPS positional or
//!           after -Property: PROP (ws? ',' ws? PROP)*, at most 16
//! PROP   := [A-Za-z_][A-Za-z0-9_]*                      at most 64 bytes
//! N      := [0-9]{1,7}
//! word   := bare | '…' | "…"
//! bare   := [A-Za-z0-9_.\/:-]+, not starting with '-'
//! ws     := (' ' | '\t')+
//! ```
//!
//! - Names and parameters compare ignoring ASCII case; each parameter at most once; parameters
//!   in any order. No short spelling but Get-Content's documented aliases: PowerShell 7's static
//!   binder cannot bind `gci -r` (`-ReadOnly` too), so the differential test could not vouch
//!   for it. A parameter is `-` and ASCII letters, then a blank, `;`, `|` or the end
//!   (`-LiteralPath:x`, `--%`, `-` alone are refused).
//! - A quoted word has a blank, `;`, `|`, `,` or the start or end on both sides (`'a'"b"` and
//!   `a"b"` are refused). Inside single quotes no `'` (no `''` escape); inside double quotes no
//!   `"`, `$` or backtick. Inside either: no smart quote (‘ ’ ‚ ‛ “ ” „, which PowerShell reads as
//!   quotes), no control or format character, no line or paragraph separator, no blank but the
//!   space.
//! - A comma only between Select-Object's properties (`gc a,b` is an array of two paths).
//! - A bare word PowerShell's number grammar could read as a number is refused: any bare path
//!   starting with a digit, or with `.` and a digit (`1kb`, `.5`, `0x1F`, `1e3`, `1d`, 7's `1s`,
//!   `1u`, `1y`, `1n`), a superset of that grammar.
//! - A path is checked by its text first ([`PathText`]); the file system checks come after
//!   (`ps_read.rs`).

use std::fmt::Write as _;

/// The longest line read; a longer one is refused.
pub(super) const MAX_LINE: usize = 4096;
/// The most statements in a line.
pub(super) const MAX_STATEMENTS: usize = 4;
/// The most properties in one Select-Object.
const MAX_PROPERTIES: usize = 16;
/// The longest property name.
const MAX_PROPERTY: usize = 64;
/// The longest path text.
const MAX_PATH_TEXT: usize = 1024;

/// What every rewritten command starts with: the two modules its cmdlets come from, Management
/// and Utility, loaded from PowerShell's own install folder (`$PSHOME`, which PowerShell sets
/// itself and the user cannot write), so a module of either name planted earlier on
/// `PSModulePath` (the user's Documents first) cannot load in its place; `Import-Module` itself
/// by its Core-qualified name. When either load fails (an execution policy, an odd layout) the
/// command stops (`exit 1`) before any cmdlet runs, never falling back to `PSModulePath`. Fixed
/// text: nothing from the agent's line is in it.
pub(crate) const PIN: &str = concat!(
    r"try { ",
    r#"Microsoft.PowerShell.Core\Import-Module -Name "$PSHOME\Modules\Microsoft.PowerShell.Management" "#,
    r"-Force -ErrorAction Stop; ",
    r#"Microsoft.PowerShell.Core\Import-Module -Name "$PSHOME\Modules\Microsoft.PowerShell.Utility" "#,
    r"-Force -ErrorAction Stop } catch { exit 1 }; ",
);

/// A line the allowlist understands: its statements, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) statements: Vec<Statement>,
}

/// One statement: a source, optionally piped to Select-Object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Statement {
    pub(crate) source: Source,
    pub(crate) select: Option<Select>,
}

/// The first stage of a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// Get-ChildItem: no path means the working folder.
    List {
        path: Option<PathText>,
        recurse: bool,
        kind: Option<Kind>,
        force: bool,
        name: bool,
    },
    /// Get-Content of one file.
    Read {
        path: PathText,
        count: Option<Count>,
        raw: bool,
    },
}

impl Source {
    /// The path the source names, if it names one.
    pub(crate) fn path(&self) -> Option<&PathText> {
        match self {
            Self::List { path, .. } => path.as_ref(),
            Self::Read { path, .. } => Some(path),
        }
    }
}

/// Get-ChildItem's `-File` or `-Directory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Directory,
}

/// Get-Content's line count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Count {
    Total(u32),
    Tail(u32),
}

/// `| Select-Object`: properties re-emitted from the checked list, and `-First`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Select {
    pub(crate) properties: Vec<String>,
    pub(crate) first: Option<u32>,
}

/// A path as the agent wrote it, checked by its text: separators made `\` (`/` on Unix), no
/// empty name, no `..`, `.` only as the first name of a relative path, no glob character
/// (`[ ]` allowed with `-LiteralPath`), no `:` but a drive's, no `~` first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathText {
    pub(crate) text: String,
    pub(crate) absolute: bool,
}

/// One token of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Bare(String),
    Quoted(String),
    Param(String),
    Comma,
    Pipe,
    Semi,
}

/// `line` read by the grammar, with Windows' path rules when `windows` (else Unix's: `/` only,
/// absolute from `/`). `None` for anything else.
pub(crate) fn parse(line: &str, windows: bool) -> Option<Line> {
    if line.len() > MAX_LINE {
        return None;
    }
    let tokens = lex(line)?;
    let mut statements = Vec::new();
    for statement in tokens.split(|token| *token == Token::Semi) {
        if statements.len() == MAX_STATEMENTS {
            return None;
        }
        statements.push(statement_of(statement, windows)?);
    }
    Some(Line { statements })
}

/// Whether `c` may stand inside a quoted word.
fn quotable(c: char) -> bool {
    const SMART: [char; 7] = [
        '\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}', '\u{201C}', '\u{201D}', '\u{201E}',
    ];
    let format = matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}');
    !(c.is_control() || (c.is_whitespace() && c != ' ') || format || SMART.contains(&c))
}

/// Whether `c` may stand in a bare word.
fn bare_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '\\' | '/' | ':' | '-')
}

/// Whether `next` may follow a word (`comma`: or a parameter, which `,` may not follow).
fn ends_token(next: Option<char>, comma: bool) -> bool {
    match next {
        None | Some(' ' | '\t' | ';' | '|') => true,
        Some(',') => comma,
        Some(_) => false,
    }
}

/// `line`'s tokens, or `None` for a character or a joint the grammar does not allow.
fn lex(line: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' => {
                chars.next();
            }
            ';' | '|' | ',' => {
                chars.next();
                tokens.push(match c {
                    ';' => Token::Semi,
                    '|' => Token::Pipe,
                    _ => Token::Comma,
                });
            }
            '\'' | '"' => {
                chars.next();
                let mut text = String::new();
                loop {
                    let inner = chars.next()?;
                    if inner == c {
                        break;
                    }
                    let refused = !quotable(inner) || (c == '"' && matches!(inner, '$' | '`'));
                    if refused {
                        return None;
                    }
                    text.push(inner);
                }
                if !ends_token(chars.peek().copied(), true) {
                    return None;
                }
                tokens.push(Token::Quoted(text));
            }
            '-' => {
                chars.next();
                let mut name = String::new();
                while let Some(&next) = chars.peek().filter(|next| next.is_ascii_alphabetic()) {
                    name.push(next);
                    chars.next();
                }
                if name.is_empty() || !ends_token(chars.peek().copied(), false) {
                    return None;
                }
                tokens.push(Token::Param(name.to_ascii_lowercase()));
            }
            _ if bare_char(c) => {
                let mut text = String::new();
                while let Some(&next) = chars.peek().filter(|next| bare_char(**next)) {
                    text.push(next);
                    chars.next();
                }
                if !ends_token(chars.peek().copied(), true) {
                    return None;
                }
                tokens.push(Token::Bare(text));
            }
            _ => return None,
        }
    }
    Some(tokens)
}

/// One statement's tokens (no `;`) read as a source and an optional select.
fn statement_of(tokens: &[Token], windows: bool) -> Option<Statement> {
    let mut stages = tokens.split(|token| *token == Token::Pipe);
    let source = source_of(stages.next()?, windows)?;
    let select = match stages.next() {
        Some(stage) => Some(select_of(stage)?),
        None => None,
    };
    if stages.next().is_some() {
        return None;
    }
    Some(Statement { source, select })
}

/// A stage's command name, lowercased, and its arguments; `None` for an empty stage or one
/// whose name is quoted.
fn named(stage: &[Token]) -> Option<(String, &[Token])> {
    match stage.split_first()? {
        (Token::Bare(name), rest) => Some((name.to_ascii_lowercase(), rest)),
        _ => None,
    }
}

/// Sets `slot`, refusing a second value.
fn once<T>(slot: &mut Option<T>, value: T) -> Option<()> {
    slot.is_none().then(|| *slot = Some(value))
}

/// A count: 1 to 7 ASCII digits, bare.
fn count_of(token: Option<&Token>) -> Option<u32> {
    match token? {
        Token::Bare(text)
            if (1..=7).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit()) =>
        {
            text.parse().ok()
        }
        Token::Bare(_)
        | Token::Quoted(_)
        | Token::Param(_)
        | Token::Comma
        | Token::Pipe
        | Token::Semi => None,
    }
}

/// A path word, checked by its text; `literal` for `-LiteralPath`.
fn path_of(token: Option<&Token>, literal: bool, windows: bool) -> Option<PathText> {
    let (text, bare) = match token? {
        Token::Bare(text) => (text, true),
        Token::Quoted(text) => (text, false),
        Token::Param(_) | Token::Comma | Token::Pipe | Token::Semi => return None,
    };
    let mut chars = text.chars();
    let first = chars.next()?;
    let number_like = first.is_ascii_digit()
        || (first == '.' && chars.next().is_some_and(|c| c.is_ascii_digit()));
    if bare && number_like {
        return None;
    }
    path_text(text, literal, windows)
}

/// `text` checked as a path ([`PathText`]).
pub(crate) fn path_text(text: &str, literal: bool, windows: bool) -> Option<PathText> {
    if text.is_empty() || text.len() > MAX_PATH_TEXT || text.starts_with('~') {
        return None;
    }
    let globs: &[char] = if literal {
        &['*', '?']
    } else {
        &['*', '?', '[', ']']
    };
    if text.contains(globs) {
        return None;
    }
    let (sep, text) = if windows {
        ('\\', text.replace('/', "\\"))
    } else if text.contains('\\') {
        return None;
    } else {
        ('/', text.to_owned())
    };
    let bytes = text.as_bytes();
    let (absolute, rest) = if windows {
        let drive = bytes.len() >= 3
            && bytes.first().is_some_and(u8::is_ascii_alphabetic)
            && bytes.get(1) == Some(&b':')
            && bytes.get(2) == Some(&b'\\');
        (drive, if drive { text.get(3..)? } else { text.as_str() })
    } else {
        let root = text.starts_with('/');
        (root, if root { text.get(1..)? } else { text.as_str() })
    };
    if rest.contains(':') || rest.starts_with(sep) {
        return None;
    }
    for (index, name) in rest.split(sep).enumerate() {
        let dot_ok = name == "." && index == 0 && !absolute;
        if name.is_empty() && !(absolute && rest.is_empty()) {
            return None;
        }
        if name == ".." || (name == "." && !dot_ok) {
            return None;
        }
    }
    Some(PathText { text, absolute })
}

/// Get-ChildItem's or Get-Content's stage.
fn source_of(stage: &[Token], windows: bool) -> Option<Source> {
    let (name, args) = named(stage)?;
    let list = match name.as_str() {
        "get-childitem" | "gci" | "ls" | "dir" => true,
        "get-content" | "gc" | "cat" | "type" => false,
        _ => return None,
    };
    let mut path = None;
    let mut switches: Vec<&str> = Vec::new();
    let mut kind = None;
    let mut count = None;
    let mut args = args.iter();
    while let Some(token) = args.next() {
        let Token::Param(param) = token else {
            once(&mut path, path_of(Some(token), false, windows)?)?;
            continue;
        };
        let switch = match (list, param.as_str()) {
            (_, "path" | "literalpath") => {
                let literal = param == "literalpath";
                once(&mut path, path_of(args.next(), literal, windows)?)?;
                continue;
            }
            (true, "recurse") => "recurse",
            (true, "file") => {
                once(&mut kind, Kind::File)?;
                continue;
            }
            (true, "directory") => {
                once(&mut kind, Kind::Directory)?;
                continue;
            }
            (true, "force") => "force",
            (true, "name") => "name",
            (false, "totalcount" | "head" | "first") => {
                once(&mut count, Count::Total(count_of(args.next())?))?;
                continue;
            }
            (false, "tail" | "last") => {
                once(&mut count, Count::Tail(count_of(args.next())?))?;
                continue;
            }
            (false, "raw") => "raw",
            _ => return None,
        };
        if switches.contains(&switch) {
            return None;
        }
        switches.push(switch);
    }
    let has = |switch: &str| switches.contains(&switch);
    if list {
        return Some(Source::List {
            path,
            recurse: has("recurse"),
            kind,
            force: has("force"),
            name: has("name"),
        });
    }
    if count.is_some() && has("raw") {
        return None;
    }
    Some(Source::Read {
        path: path?,
        count,
        raw: has("raw"),
    })
}

/// A property name.
fn property(token: Option<&Token>) -> Option<String> {
    let Some(Token::Bare(name)) = token else {
        return None;
    };
    let mut chars = name.chars();
    let head = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    let ok =
        head && name.len() <= MAX_PROPERTY && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then(|| name.clone())
}

/// Select-Object's stage.
fn select_of(stage: &[Token]) -> Option<Select> {
    let (name, args) = named(stage)?;
    if !matches!(name.as_str(), "select-object" | "select") {
        return None;
    }
    let mut properties: Option<Vec<String>> = None;
    let mut first = None;
    let mut args = args.iter().peekable();
    while let Some(token) = args.next() {
        let list_start = match token {
            Token::Param(param) if param == "first" => {
                once(&mut first, count_of(args.next())?)?;
                continue;
            }
            Token::Param(param) if param == "property" => args.next(),
            Token::Bare(_) => Some(token),
            Token::Quoted(_) | Token::Param(_) | Token::Comma | Token::Pipe | Token::Semi => {
                return None;
            }
        };
        let mut list = vec![property(list_start)?];
        while args.next_if_eq(&&Token::Comma).is_some() {
            list.push(property(args.next())?);
        }
        if list.len() > MAX_PROPERTIES {
            return None;
        }
        once(&mut properties, list)?;
    }
    let properties = properties.unwrap_or_default();
    (!properties.is_empty() || first.is_some()).then_some(Select { properties, first })
}

/// `path` as a single-quoted PowerShell string, `'` doubled. `None` for a path with a smart
/// single quote (PowerShell reads ‘ ’ ‚ ‛ as `'` too), a control or format character, a line
/// or paragraph separator, or one that is not UTF-8.
pub(crate) fn quoted(path: &str) -> Option<String> {
    if !path.chars().all(quotable) {
        return None;
    }
    Some(format!("'{}'", path.replace('\'', "''")))
}

/// The command written in place of `line`, each statement's path the canonical one in
/// `paths` (same order, already quoted with [`quoted`]): [`PIN`], then each statement with
/// module-qualified names, full parameter names and `-LiteralPath`, joined by `; `.
pub(crate) fn rewrite(line: &Line, paths: &[String]) -> Option<String> {
    if paths.len() != line.statements.len() {
        return None;
    }
    let mut out = PIN.to_owned();
    for (index, (statement, path)) in line.statements.iter().zip(paths).enumerate() {
        if index > 0 {
            out.push_str("; ");
        }
        match &statement.source {
            Source::List {
                recurse,
                kind,
                force,
                name,
                ..
            } => {
                let _infallible = write!(
                    out,
                    r"Microsoft.PowerShell.Management\Get-ChildItem -LiteralPath {path}"
                );
                for (on, text) in [
                    (*recurse, " -Recurse"),
                    (*kind == Some(Kind::File), " -File"),
                    (*kind == Some(Kind::Directory), " -Directory"),
                    (*force, " -Force"),
                    (*name, " -Name"),
                ] {
                    if on {
                        out.push_str(text);
                    }
                }
            }
            Source::Read { count, raw, .. } => {
                let _infallible = write!(
                    out,
                    r"Microsoft.PowerShell.Management\Get-Content -LiteralPath {path}"
                );
                match count {
                    Some(Count::Total(n)) => {
                        let _infallible = write!(out, " -TotalCount {n}");
                    }
                    Some(Count::Tail(n)) => {
                        let _infallible = write!(out, " -Tail {n}");
                    }
                    None => {}
                }
                if *raw {
                    out.push_str(" -Raw");
                }
            }
        }
        if let Some(select) = &statement.select {
            out.push_str(r" | Microsoft.PowerShell.Utility\Select-Object");
            if !select.properties.is_empty() {
                let _infallible = write!(out, " -Property {}", select.properties.join(", "));
            }
            if let Some(n) = select.first {
                let _infallible = write!(out, " -First {n}");
            }
        }
    }
    Some(out)
}
