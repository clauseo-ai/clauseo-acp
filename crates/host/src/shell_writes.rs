//! The files a turn's shell commands write, for the after-turn checkpoint to tell a file the
//! agent made through a shell (pandoc's `-o "Reply.docx"`, `echo x > notes.md`, `cp a b`) from
//! one the lawyer saved meanwhile: a new file is the agent's own only when a tool call names it
//! or such a command writes it (`clauseo_matters::Matter::after_turn_named`).
//!
//! Only the command text in a shell call's input is read, never its output: a listing or a
//! `grep` names files it does not write. Only write targets count: `>` and `>>` redirects,
//! `-o` and `--output` values, `tee`'s files, and the last argument of `cp` and `mv`. The
//! command is split as a shell does (quotes kept together) into segments at `;`, `&&`, `||`, `|`
//! and new lines; leading `cd <dir>` segments set the folder later ones run in. Each target is
//! joined to that folder (the call's `cwd` when its input says, else the matter's folder) and
//! normalised, so it compares with a file's whole path in the matter. Anything unclear (an
//! expansion, a glob, a later `cd`, a target ending in `/`) writes nothing. Kept in memory for
//! the turn only, never logged.

use std::collections::BTreeSet;
use std::path::Path;

/// The most targets one turn keeps.
const MAX_TARGETS: usize = 1024;

/// Commands whose `-o` is no output file.
const NOT_OUTPUT: [&str; 10] = [
    "grep", "egrep", "fgrep", "rg", "ls", "ps", "find", "test", "[", "ssh",
];

/// Adds the targets of the command in a shell call's input (its JSON text): a `command` string,
/// or an argument list (`["bash", "-lc", "<script>"]`, or the program's own arguments), run in
/// its `cwd` or `workdir` when the input gives one. Input that is not JSON adds nothing.
pub(crate) fn add_input(targets: &mut BTreeSet<String>, raw_input: &str) {
    let Ok(serde_json::Value::Object(input)) = serde_json::from_str(raw_input) else {
        return;
    };
    let cwd = ["cwd", "workdir"]
        .iter()
        .find_map(|key| input.get(*key).and_then(serde_json::Value::as_str))
        .unwrap_or("");
    match input.get("command") {
        Some(serde_json::Value::String(script)) => add_script(targets, cwd, script),
        Some(serde_json::Value::Array(items)) => {
            let Some(argv) = items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect::<Option<Vec<String>>>()
            else {
                return;
            };
            match argv.as_slice() {
                [shell, flag, script]
                    if ["-c", "-lc", "-ic"].contains(&flag.as_str())
                        && ["sh", "bash", "zsh", "dash"].contains(&base(shell)) =>
                {
                    add_script(targets, cwd, script);
                }
                _ => {
                    let words = argv.into_iter().map(Word::plain).collect();
                    add_segment(
                        targets,
                        cwd,
                        &Segment {
                            words,
                            redirects: Vec::new(),
                        },
                    );
                }
            }
        }
        _ => {}
    }
}

/// Adds the targets of the shell script `script`, run in `cwd` (`""`: the matter's folder).
pub(crate) fn add_script(targets: &mut BTreeSet<String>, cwd: &str, script: &str) {
    let mut cwd = cwd.to_owned();
    let mut leading = true;
    for segment in segments(script) {
        let first = segment.words.first().map(|word| word.text.as_str());
        if first == Some("cd") {
            // A leading `cd` sets the folder of what follows; a later one leaves it unknown.
            match (leading, segment.words.as_slice()) {
                (true, [_, dir]) if dir.clear && segment.redirects.is_empty() => {
                    let Some(joined) = join(&cwd, &dir.text) else {
                        return;
                    };
                    cwd = joined;
                    continue;
                }
                _ => return,
            }
        }
        leading = false;
        add_segment(targets, &cwd, &segment);
    }
}

/// The path in the matter `folder` of `target` (relative to the folder, or absolute), or `None`
/// when it lies outside it.
pub(crate) fn in_matter(folder: &Path, target: &str) -> Option<String> {
    if !is_absolute(target) {
        return Some(target.to_owned());
    }
    let folder = join("", &folder.to_string_lossy())?;
    let rest = target.strip_prefix(folder.as_str())?.strip_prefix('/')?;
    (!rest.is_empty()).then(|| rest.to_owned())
}

/// One word of a command, and whether it is plain text: no expansion, glob or `~`.
#[derive(Debug)]
struct Word {
    text: String,
    clear: bool,
}

impl Word {
    fn plain(text: String) -> Self {
        Self { text, clear: true }
    }
}

/// One simple command: its words, and the targets of its `>` and `>>` redirects.
#[derive(Debug, Default)]
struct Segment {
    words: Vec<Word>,
    redirects: Vec<Word>,
}

/// Adds one simple command's targets, run in `cwd`.
fn add_segment(targets: &mut BTreeSet<String>, cwd: &str, segment: &Segment) {
    let mut found: Vec<&Word> = segment.redirects.iter().collect();
    let words = &segment.words;
    // Leading assignments and wrappers that run the rest as the command.
    let start = words
        .iter()
        .position(|word| {
            !is_assignment(&word.text)
                && !["env", "command", "time", "nohup", "sudo"].contains(&word.text.as_str())
        })
        .unwrap_or(words.len());
    let argv = words.get(start..).unwrap_or_default();
    if let Some((program, args)) = argv.split_first() {
        let program = base(&program.text);
        if !NOT_OUTPUT.contains(&program) {
            for (i, arg) in args.iter().enumerate() {
                if arg.text == "-o" || arg.text == "--output" {
                    found.extend(args.get(i + 1));
                }
            }
        }
        let values = args.iter().filter(|arg| !arg.text.starts_with('-'));
        match program {
            "tee" => found.extend(values),
            "cp" | "mv"
                if !args.iter().any(|arg| {
                    arg.text.starts_with("-t") || arg.text.starts_with("--target-directory")
                }) =>
            {
                let values: Vec<&Word> = values.collect();
                if values.len() >= 2 {
                    found.extend(values.last().copied());
                }
            }
            _ => {}
        }
        // `--output=Reply.docx`
        for arg in args {
            if let Some(value) = arg.text.strip_prefix("--output=")
                && arg.clear
                && !value.is_empty()
            {
                add(targets, cwd, value);
            }
        }
    }
    for word in found {
        if word.clear {
            add(targets, cwd, &word.text);
        }
    }
}

fn add(targets: &mut BTreeSet<String>, cwd: &str, target: &str) {
    if targets.len() >= MAX_TARGETS || target.ends_with(['/', '\\']) {
        return;
    }
    if let Some(path) = join(cwd, target)
        && !path.is_empty()
    {
        targets.insert(path);
    }
}

/// `target` joined to `cwd` and normalised: separators as `/`, no `.` or empty part, each `..`
/// taking off the part before it. An absolute result keeps its root (`/`, `C:/`, `//`); a
/// relative one that climbs out of the matter's folder is `None`.
fn join(cwd: &str, target: &str) -> Option<String> {
    let whole = if is_absolute(target) || cwd.is_empty() {
        target.to_owned()
    } else {
        format!("{cwd}/{target}")
    };
    let unified = whole.replace('\\', "/");
    let (root, rest) = if let Some(rest) = unified.strip_prefix("//") {
        ("//".to_owned(), rest)
    } else if let Some(rest) = unified.strip_prefix('/') {
        ("/".to_owned(), rest)
    } else if let Some((drive, rest)) = drive_of(&unified) {
        (format!("{drive}:"), rest.trim_start_matches('/'))
    } else {
        (String::new(), unified.as_str())
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() && root.is_empty() {
                    return None;
                }
            }
            _ => parts.push(part),
        }
    }
    let joined = parts.join("/");
    Some(match root.as_str() {
        "" => joined,
        "/" | "//" => format!("{root}{joined}"),
        drive => format!("{drive}/{joined}"),
    })
}

/// A Windows path's drive letter and what follows its colon: `C:\Matter` is `('C', "\Matter")`.
fn drive_of(path: &str) -> Option<(char, &str)> {
    let mut chars = path.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic() => Some((letter, chars.as_str())),
        _ => None,
    }
}

fn is_absolute(path: &str) -> bool {
    path.starts_with(['/', '\\']) || drive_of(path).is_some()
}

/// The program's name without its folder.
fn base(program: &str) -> &str {
    program.rsplit(['/', '\\']).next().unwrap_or(program)
}

/// `NAME=value`.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit())
    })
}

/// The words of the segment being read, and where the word being read goes.
#[derive(Default)]
struct Reader {
    out: Vec<Segment>,
    segment: Segment,
    word: String,
    /// A word has begun (an empty quoted word counts).
    started: bool,
    /// It holds no expansion, glob or `~` so far.
    unclear: bool,
    /// It is a redirect's target.
    redirect: bool,
}

impl Reader {
    fn push(&mut self, c: char) {
        self.started = true;
        self.word.push(c);
    }

    fn end_word(&mut self) {
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

    fn end_segment(&mut self) {
        self.end_word();
        self.redirect = false;
        let segment = std::mem::take(&mut self.segment);
        if !segment.words.is_empty() || !segment.redirects.is_empty() {
            self.out.push(segment);
        }
    }
}

/// The simple commands of `script`, as a POSIX shell splits it.
fn segments(script: &str) -> Vec<Segment> {
    let mut r = Reader::default();
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
                while chars.next_if(|d| *d == ' ' || *d == '<').is_some() {}
                while chars
                    .next_if(|d| !d.is_whitespace() && !";&|<>".contains(*d))
                    .is_some()
                {}
            }
            ';' | '&' | '|' | '\n' | '(' | ')' => {
                if matches!(c, '&' | '|') && chars.peek() == Some(&c) {
                    chars.next();
                }
                r.end_segment();
            }
            c if c.is_whitespace() => r.end_word(),
            _ => r.push(c),
        }
    }
    r.end_segment();
    r.out
}

#[cfg(test)]
mod tests;
