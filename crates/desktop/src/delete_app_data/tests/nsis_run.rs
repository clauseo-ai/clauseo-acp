//! A small interpreter of the NSIS and `LogicLib` lines the hooks' registry and message steps
//! use, for tests that run those steps on a fake registry instead of pinning their text. It reads
//! the lines from the sources, keeps what `!if`, `!ifdef`, `!ifndef` and `!else` select for a mode,
//! and fails on any line it doesn't know, so a step that grows past it fails its test.

use std::collections::HashMap;

/// A record value as `ReadRegStr` reads it: text, or a value it can't read (another type).
#[derive(Clone, Copy)]
pub(super) enum Value {
    Text(&'static str),
    Unreadable,
}

/// The record key's values in enumeration order, or `None` for no key.
pub(super) type Record = Option<Vec<(&'static str, Value)>>;

/// A line, an `${If}` chain (each arm's conditions, joined by `If`, `IfNot`, `Or` or `And`, then
/// its nodes, then the `${Else}` arm's), or a `${Do}` loop.
pub(super) enum Node {
    Line(String),
    If(Vec<(Vec<String>, Vec<Node>)>, Option<Vec<Node>>),
    Do(Vec<Node>),
}

/// Every `!define NAME VALUE` in `code`, the first of each name, quotes dropped.
pub(super) fn defines(code: &[String]) -> HashMap<String, String> {
    let mut defines = HashMap::new();
    for line in code {
        if let Some((name, value)) = line
            .strip_prefix("!define ")
            .and_then(|rest| rest.split_once(' '))
        {
            defines
                .entry(name.to_owned())
                .or_insert_with(|| value.trim_matches('"').to_owned());
        }
    }
    defines
}

/// The lines `!if`, `!ifdef`, `!ifndef`, `!ifmacrodef` (always true) and `!else` keep under
/// `defines`, those lines themselves left out.
pub(super) fn preprocess(lines: &[String], defines: &HashMap<String, String>) -> Vec<String> {
    let expand = |word: &str| {
        let mut word = word.trim_matches('"').to_owned();
        for (name, value) in defines {
            word = word.replace(&format!("${{{name}}}"), value);
        }
        word
    };
    let mut kept = Vec::new();
    // Each open block: whether its lines are kept.
    let mut open: Vec<bool> = Vec::new();
    for line in lines {
        let outer = open.iter().all(|&on| on);
        let parts = words(line);
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        match parts.as_slice() {
            ["!if", a, op, b] => {
                let (a, b) = (expand(a), expand(b));
                let number = |text: &str| text.parse::<i64>().unwrap();
                open.push(match *op {
                    "==" => a == b,
                    "!=" => a != b,
                    ">" => number(&a) > number(&b),
                    _ => panic!("an !if this interpreter doesn't know: {line}"),
                });
            }
            ["!ifdef", name] => open.push(defines.contains_key(*name)),
            ["!ifndef", name] => open.push(!defines.contains_key(*name)),
            ["!ifmacrodef", _] => open.push(true),
            ["!else"] => {
                let last = open.pop().expect("an !else without !if");
                open.push(!last);
            }
            ["!endif"] => {
                open.pop().expect("an !endif without !if");
            }
            _ if outer => kept.push(line.clone()),
            _ => {}
        }
    }
    assert!(open.is_empty(), "an !if without !endif");
    kept
}

/// Parses all of `lines`.
pub(super) fn parse_all(lines: &[String]) -> Vec<Node> {
    parse(lines, &mut 0, &[])
}

/// Parses from `lines[*at]` up to (not past) a line that starts with one of `ends`.
fn parse(lines: &[String], at: &mut usize, ends: &[&str]) -> Vec<Node> {
    let mut nodes = Vec::new();
    while let Some(line) = lines.get(*at) {
        if ends
            .iter()
            .any(|end| line == end || line.starts_with(&format!("{end} ")))
        {
            return nodes;
        }
        *at += 1;
        let first = (line.strip_prefix("${If} ").map(|cond| format!("If {cond}"))).or_else(|| {
            line.strip_prefix("${IfNot} ")
                .map(|cond| format!("IfNot {cond}"))
        });
        if let Some(first) = first {
            let mut arms = Vec::new();
            let mut conds = vec![first];
            loop {
                while let Some(line) = lines.get(*at) {
                    if let Some(cond) = line.strip_prefix("${OrIf} ") {
                        conds.push(format!("Or {cond}"));
                    } else if let Some(cond) = line.strip_prefix("${AndIf} ") {
                        conds.push(format!("And {cond}"));
                    } else {
                        break;
                    }
                    *at += 1;
                }
                let body = parse(
                    lines,
                    at,
                    &["${ElseIf}", "${ElseIfNot}", "${Else}", "${EndIf}"],
                );
                arms.push((std::mem::take(&mut conds), body));
                let end = &lines[*at];
                *at += 1;
                if let Some(cond) = end.strip_prefix("${ElseIf} ") {
                    conds.push(format!("If {cond}"));
                } else if let Some(cond) = end.strip_prefix("${ElseIfNot} ") {
                    conds.push(format!("IfNot {cond}"));
                } else if end == "${Else}" {
                    let otherwise = parse(lines, at, &["${EndIf}"]);
                    *at += 1;
                    nodes.push(Node::If(arms, Some(otherwise)));
                    break;
                } else {
                    nodes.push(Node::If(arms, None));
                    break;
                }
            }
        } else if line == "${Do}" {
            let body = parse(lines, at, &["${Loop}"]);
            *at += 1;
            nodes.push(Node::Do(body));
        } else {
            nodes.push(Node::Line(line.clone()));
        }
    }
    assert!(ends.is_empty(), "no {ends:?}");
    nodes
}

/// Splits on spaces outside quotes, double or single; the outer quotes are dropped, and a quote
/// of the other kind inside them is kept as text.
pub(super) fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted: Option<char> = None;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' | '\'' if quoted.is_none() => {
                quoted = Some(c);
                any = true;
            }
            '"' | '\'' if quoted == Some(c) => {
                quoted = None;
                any = true;
            }
            ' ' if quoted.is_none() => {
                if any {
                    words.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            _ => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        words.push(word);
    }
    words
}

/// The `${If}` chain in `nodes`, at any depth, whose first condition is `first`.
pub(super) fn chain<'a>(nodes: &'a [Node], first: &str) -> Option<&'a Node> {
    nodes.iter().find_map(|node| match node {
        Node::If(arms, otherwise) => {
            if arms[0].0 == [first] {
                return Some(node);
            }
            arms.iter()
                .find_map(|(_, body)| chain(body, first))
                .or_else(|| otherwise.as_ref().and_then(|body| chain(body, first)))
        }
        Node::Do(body) => chain(body, first),
        Node::Line(_) => None,
    })
}

/// Every line of `nodes`, in order, whatever arm it is in.
pub(super) fn lines(nodes: &[Node]) -> Vec<&str> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            Node::Line(line) => out.push(line.as_str()),
            Node::If(arms, otherwise) => {
                for (_, body) in arms {
                    out.extend(lines(body));
                }
                if let Some(body) = otherwise {
                    out.extend(lines(body));
                }
            }
            Node::Do(body) => out.extend(lines(body)),
        }
    }
    out
}

/// The machine: the defines, variables, error flag and stack, the record under
/// `HKLM\SOFTWARE\Clauseo\Sandbox` and the lawyer's key under `HKCU\Software\Clauseo`, whether
/// other keys a test sets (`keys`, by root and expanded key), whether the run is silent, the user's SID for `CLAUSEO_OWN_SID`, the macros it runs inline (and the
/// record each leaves), what it said (`DetailPrint`, `MessageBox`, `SetErrorLevel`), and which
/// stubbed macros it reached.
pub(super) struct Machine {
    pub(super) defines: HashMap<String, String>,
    pub(super) vars: HashMap<String, String>,
    pub(super) error: bool,
    pub(super) stack: Vec<String>,
    pub(super) record: Record,
    /// The lawyer's key, `HKCU\Software\Clauseo`, where the app writes `UiLanguage`.
    pub(super) user: Record,
    /// Any other key, by `ROOT\key` (`SHCTX\Software\Clauseo\Clauseo`, say).
    pub(super) keys: HashMap<String, Vec<(&'static str, Value)>>,
    pub(super) silent: bool,
    pub(super) own_sid: &'static str,
    pub(super) inline: HashMap<String, Vec<Node>>,
    /// The record an inlined macro leaves behind it, as a refresh landing while it runs would.
    pub(super) after: HashMap<String, Record>,
    pub(super) said: Vec<String>,
    pub(super) reached: Vec<String>,
}

impl Machine {
    pub(super) fn new(defines: HashMap<String, String>, record: Record) -> Self {
        Self {
            defines,
            vars: HashMap::new(),
            error: false,
            stack: Vec::new(),
            record,
            user: None,
            keys: HashMap::new(),
            silent: false,
            own_sid: "",
            inline: HashMap::new(),
            after: HashMap::new(),
            said: Vec::new(),
            reached: Vec::new(),
        }
    }

    /// `${NAME}` from the defines, `$R0`..`$R9` and `$0`..`$9` from the variables, as is any other
    /// `$name` (`$INSTDIR`, a `Var`); `$(name)`, a language string, stays as it is.
    fn expand(&self, word: &str) -> String {
        let mut out = String::new();
        let mut rest = word;
        while let Some(at) = rest.find('$') {
            out.push_str(&rest[..at]);
            rest = &rest[at..];
            if let Some(tail) = rest.strip_prefix("${") {
                let (name, after) = tail.split_once('}').expect("an unclosed ${");
                out.push_str(
                    self.defines
                        .get(name)
                        .unwrap_or_else(|| panic!("no define {name}")),
                );
                rest = after;
            } else if rest.starts_with("$(") {
                let end = rest.find(')').expect("an unclosed $(") + 1;
                out.push_str(&rest[..end]);
                rest = &rest[end..];
            } else {
                let tail = &rest[1..];
                let register = tail.starts_with(|c: char| c.is_ascii_digit())
                    || (tail.starts_with('R')
                        && tail[1..].starts_with(|c: char| c.is_ascii_digit()));
                let len = if register {
                    if tail.starts_with('R') { 3 } else { 2 }
                } else {
                    1 + tail
                        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                        .unwrap_or(tail.len())
                };
                let name = &rest[..len];
                out.push_str(self.vars.get(name).map_or("", String::as_str));
                rest = &rest[len..];
            }
        }
        out.push_str(rest);
        out
    }

    fn holds(&self, conds: &[String]) -> bool {
        conds.iter().fold(false, |held, cond| {
            let (join, cond) = cond.split_once(' ').unwrap();
            let value = self.test(cond);
            match join {
                "If" => value,
                "IfNot" => !value,
                "Or" => held || value,
                "And" => held && value,
                _ => unreachable!(),
            }
        })
    }

    fn test(&self, cond: &str) -> bool {
        match cond {
            "${Errors}" => return self.error,
            "${Silent}" => return self.silent,
            _ => {}
        }
        let parts = words(cond);
        let [a, op, b] = parts.as_slice() else {
            panic!("a condition this interpreter doesn't know: {cond}")
        };
        let (a, b) = (self.expand(a), self.expand(b));
        let number = |text: &str| text.parse::<i64>().unwrap_or(0);
        match op.as_str() {
            "==" => a.eq_ignore_ascii_case(&b),
            "!=" => !a.eq_ignore_ascii_case(&b),
            "S==" => a == b,
            "S!=" => a != b,
            "=" => number(&a) == number(&b),
            "<>" => number(&a) != number(&b),
            _ => panic!("an operator this interpreter doesn't know: {cond}"),
        }
    }

    fn key(&self, root: &str, key: &str) -> Option<&Vec<(&'static str, Value)>> {
        let key = self.expand(key);
        if let Some(values) = self.keys.get(&format!("{root}\\{key}")) {
            return Some(values);
        }
        match (root, key.as_str()) {
            ("HKLM", "SOFTWARE\\Clauseo\\Sandbox") => self.record.as_ref(),
            ("HKCU", "Software\\Clauseo") => self.user.as_ref(),
            (root, key) => panic!("a key this interpreter doesn't know: {root}\\{key}"),
        }
    }

    /// Sets a variable, as a test seeds one.
    pub(super) fn set_var(&mut self, var: &str, value: &str) {
        self.set(var, value.to_owned());
    }

    fn set(&mut self, var: &str, value: String) {
        self.vars.insert(var.to_owned(), value);
    }

    fn line(&mut self, line: &str) {
        let parts = words(line);
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        match parts.as_slice() {
            ["StrCpy", var, value] => self.set(var, self.expand(value)),
            ["StrCpy", var, value, len] => {
                let text = str_copy(&self.expand(value), &self.expand(len), "0");
                self.set(var, text);
            }
            ["StrCpy", var, value, len, offset] => {
                let text = str_copy(&self.expand(value), &self.expand(len), &self.expand(offset));
                self.set(var, text);
            }
            ["ClearErrors"] => self.error = false,
            ["StrLen", var, text] => {
                let len = self.expand(text).chars().count();
                self.set(var, len.to_string());
            }
            ["SetRegView", "64" | "lastused"] | ["BringToFront"] => {}
            ["Push", value] => self.stack.push(self.expand(value)),
            ["Pop", var] => {
                let value = self.stack.pop().expect("a Pop on an empty stack");
                self.set(var, value);
            }
            ["EnumRegValue", var, root, key, index] => {
                let index: usize = self.expand(index).parse().unwrap();
                let name = self
                    .key(root, key)
                    .and_then(|values| values.get(index))
                    .map(|(name, _)| *name);
                self.error |= name.is_none();
                self.set(var, name.unwrap_or("").to_owned());
            }
            ["ReadRegStr", var, root, key, name] => {
                let name = self.expand(name);
                let found = self.key(root, key).and_then(|values| {
                    values
                        .iter()
                        .find(|(have, _)| have.eq_ignore_ascii_case(&name))
                        .map(|(_, value)| *value)
                });
                let text = match found {
                    Some(Value::Text(text)) => text,
                    Some(Value::Unreadable) | None => {
                        self.error = true;
                        ""
                    }
                };
                self.set(var, text.to_owned());
            }
            ["IntOp", var, a, "+", b] => {
                let a: i64 = self.expand(a).parse().unwrap();
                let b: i64 = self.expand(b).parse().unwrap();
                self.set(var, (a + b).to_string());
            }
            ["IntOp", var, a, "&", b] => {
                let a: i64 = self.expand(a).parse().unwrap();
                let b: i64 = self.expand(b).parse().unwrap();
                self.set(var, (a & b).to_string());
            }
            // A program started and waited for: said, and its exit code 0.
            ["ExecWait", command, var] => {
                let command = self.expand(command);
                self.said.push(format!("ExecWait {command}"));
                self.set(var, "0".to_owned());
            }
            // A jump, said; the tests put it last in its arm.
            ["Goto", label] => self.said.push(format!("Goto {label}")),
            ["DetailPrint", text] => {
                let text = self.expand(text);
                self.said.push(format!("DetailPrint {text}"));
            }
            ["MessageBox", _, text] => {
                let text = self.expand(text);
                self.said.push(format!("MessageBox {text}"));
            }
            ["SetErrorLevel", code] => {
                let code = self.expand(code);
                self.said.push(format!("SetErrorLevel {code}"));
            }
            // The token's user, as the test sets it.
            ["!insertmacro", "CLAUSEO_OWN_SID"] => {
                self.reached.push("CLAUSEO_OWN_SID".to_owned());
                self.set("$R2", self.own_sid.to_owned());
            }
            ["!insertmacro", name] if self.inline.contains_key(*name) => {
                let nodes = self.inline.remove(*name).unwrap();
                let exited = self.run(&nodes);
                assert!(!exited, "an ${{ExitDo}} out of {name}");
                self.inline.insert((*name).to_owned(), nodes);
                if let Some(record) = self.after.remove(*name) {
                    self.record = record;
                }
            }
            ["!insertmacro", name] => self.reached.push((*name).to_owned()),
            _ => panic!("a line this interpreter doesn't know: {line}"),
        }
    }

    /// Runs `nodes`; true when an `${ExitDo}` leaves them.
    pub(super) fn run(&mut self, nodes: &[Node]) -> bool {
        for node in nodes {
            match node {
                Node::Line(line) if line == "${ExitDo}" => return true,
                Node::Line(line) => self.line(line),
                Node::If(arms, otherwise) => {
                    let chosen = arms
                        .iter()
                        .find(|(conds, _)| self.holds(conds))
                        .map(|(_, body)| body)
                        .or(otherwise.as_ref());
                    if chosen.is_some_and(|body| self.run(body)) {
                        return true;
                    }
                }
                Node::Do(body) => {
                    let mut turns = 0;
                    while !self.run(body) {
                        turns += 1;
                        assert!(turns < 100, "a ${{Do}} that doesn't end");
                    }
                }
            }
        }
        false
    }
}

/// NSIS's `StrCpy var text len offset`: `offset` characters in (from the end when negative), then
/// `len` characters (all when empty; all but that many from the end when negative).
fn str_copy(text: &str, len: &str, offset: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let count = i64::try_from(chars.len()).unwrap();
    let offset: i64 = offset.parse().unwrap();
    let start = if offset < 0 { count + offset } else { offset }.clamp(0, count);
    let rest = count - start;
    let take = match len {
        "" => rest,
        len => {
            let len: i64 = len.parse().unwrap();
            if len < 0 { rest + len } else { len }
        }
    }
    .clamp(0, rest);
    let (start, take) = (
        usize::try_from(start).unwrap(),
        usize::try_from(take).unwrap(),
    );
    chars[start..start + take].iter().collect()
}
