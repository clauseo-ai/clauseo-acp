//! The record check before any `File` into the admin folder (`CLAUSEO_RECORD_CHECK`,
//! `admin_copy.nsh`), run on a fake registry: its lines are read from the source and run by a
//! small interpreter of the NSIS and LogicLib they use, which fails on any other line. No record
//! goes ahead (0); another owner is refused (1); a record holding an owner or version value that
//! can't be read as text is refused (3), as `--adopt` fails on it. Then both callers, the per-user
//! elevated instance and the per-machine installer, write the copy only when the check gives 0.

use std::collections::HashMap;

use super::admin_copy::{ASK, admin_code, assert_run, macro_body};

const ME: &str = "S-1-5-21-1-2-3-1001";
const OTHER: &str = "S-1-5-21-1-2-3-1002";
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A record value as `ReadRegStr` reads it: text, or a value it can't read (another type).
#[derive(Clone, Copy)]
enum Value {
    Text(&'static str),
    Unreadable,
}

/// The record key's values in enumeration order, or `None` for no key.
type Record = Option<Vec<(&'static str, Value)>>;

/// A line, an `${If}` chain (each arm's conditions, joined by `If`, `Or` or `And`, then its
/// nodes, then the `${Else}` arm's), or a `${Do}` loop.
enum Node {
    Line(String),
    If(Vec<(Vec<String>, Vec<Node>)>, Option<Vec<Node>>),
    Do(Vec<Node>),
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

/// Splits on spaces outside double quotes; the quotes are dropped.
fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            ' ' if !quoted => {
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

struct Machine {
    defines: HashMap<String, String>,
    vars: HashMap<String, String>,
    error: bool,
    record: Record,
    newer_checked: bool,
}

impl Machine {
    /// `${NAME}` from the defines, `$R0`..`$R9` and `$0`..`$9` from the variables.
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
            } else {
                let len = if rest[1..].starts_with('R') { 3 } else { 2 };
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
        if cond == "${Errors}" {
            return self.error;
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
            "S!=" => a != b,
            "=" => number(&a) == number(&b),
            _ => panic!("an operator this interpreter doesn't know: {cond}"),
        }
    }

    fn key(&self, root: &str, key: &str) -> Option<&Vec<(&'static str, Value)>> {
        assert_eq!(root, "HKLM");
        assert_eq!(self.expand(key), "SOFTWARE\\Clauseo\\Sandbox");
        self.record.as_ref()
    }

    fn line(&mut self, line: &str) {
        let parts = words(line);
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        match parts.as_slice() {
            ["StrCpy", var, value] => {
                let value = self.expand(value);
                self.vars.insert((*var).to_owned(), value);
            }
            ["ClearErrors"] => self.error = false,
            ["EnumRegValue", var, root, key, index] => {
                let index: usize = self.expand(index).parse().unwrap();
                let name = self
                    .key(root, key)
                    .and_then(|values| values.get(index))
                    .map(|(name, _)| *name);
                self.error |= name.is_none();
                self.vars
                    .insert((*var).to_owned(), name.unwrap_or("").to_owned());
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
                self.vars.insert((*var).to_owned(), text.to_owned());
            }
            ["IntOp", var, a, "+", b] => {
                let sum =
                    self.expand(a).parse::<i64>().unwrap() + self.expand(b).parse::<i64>().unwrap();
                self.vars.insert((*var).to_owned(), sum.to_string());
            }
            // The version comparison has its own test (admin_copy.rs).
            ["!insertmacro", "CLAUSEO_NEWER_THAN_THIS"] => self.newer_checked = true,
            _ => panic!("a line this interpreter doesn't know: {line}"),
        }
    }

    /// Runs `nodes`; true when an `${ExitDo}` leaves them.
    fn run(&mut self, nodes: &[Node]) -> bool {
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

/// `CLAUSEO_RECORD_CHECK OWNER` on `record`: its `$R1`, and whether it reached the version check.
fn check(owner: &str, record: Record) -> (i64, bool) {
    let code = admin_code();
    let mut defines: HashMap<String, String> = code
        .iter()
        .filter_map(|line| line.strip_prefix("!define CLAUSEO_RECORD_"))
        .map(|rest| {
            let (name, value) = rest.split_once(' ').unwrap();
            (
                format!("CLAUSEO_RECORD_{name}"),
                value.trim_matches('"').to_owned(),
            )
        })
        .collect();
    defines.insert("OWNER".to_owned(), owner.to_owned());
    let body = &macro_body(&code, "CLAUSEO_RECORD_CHECK")[1..];
    let nodes = parse(body, &mut 0, &[]);
    let mut machine = Machine {
        defines,
        vars: HashMap::new(),
        error: false,
        record,
        newer_checked: false,
    };
    machine.run(&nodes);
    let r1 = machine.vars.get("$R1").expect("$R1 set").parse().unwrap();
    (r1, machine.newer_checked)
}

fn text(name: &'static str, value: &'static str) -> (&'static str, Value) {
    (name, Value::Text(value))
}

#[test]
fn no_record_goes_ahead() {
    for owner in [ME, ""] {
        // No key (the first install), or a key with no values.
        assert_eq!(check(owner, None), (0, false), "{owner}");
        assert_eq!(check(owner, Some(Vec::new())), (0, false), "{owner}");
    }
    // This user's own record, read whole.
    let own = vec![
        text("AdminCopySha256", SHA),
        text("AdminCopyOwner", ME),
        text("OwnerSid", ME),
        text("AdminCopyVersion", "0.1.0"),
    ];
    assert_eq!(check(ME, Some(own)), (0, true));
    // Per machine: the copy's values with no OwnerSid (nobody set up yet, or after a removal), or
    // a user's set-up beside a per-machine copy.
    let machine = vec![
        text("AdminCopySha256", SHA),
        text("AdminCopyOwner", "machine"),
        text("AdminCopyVersion", "0.1.0"),
    ];
    assert_eq!(check("", Some(machine.clone())), (0, true));
    let mut set_up = machine;
    set_up.push(text("OwnerSid", OTHER));
    assert_eq!(check("", Some(set_up)), (0, true));
    // A value the check doesn't read, unreadable: --adopt writes it again.
    assert_eq!(
        check(ME, Some(vec![("AdminCopySha256", Value::Unreadable)])),
        (0, false)
    );
}

#[test]
fn a_record_naming_another_owner_is_refused() {
    for record in [
        vec![text("AdminCopySha256", SHA), text("AdminCopyOwner", OTHER)],
        vec![
            text("AdminCopySha256", SHA),
            text("AdminCopyOwner", "machine"),
        ],
        vec![text("OwnerSid", OTHER)],
        // Compared exactly: a SID in another case isn't this one.
        vec![text("AdminCopyOwner", "s-1-5-21-1-2-3-1001")],
    ] {
        assert_eq!(check(ME, Some(record)), (1, false));
    }
    assert_eq!(
        check(
            "",
            Some(vec![
                text("AdminCopySha256", SHA),
                text("AdminCopyOwner", ME)
            ])
        ),
        (1, false)
    );
}

#[test]
fn a_record_whose_owner_or_version_cant_be_read_is_refused() {
    for owner in [ME, ""] {
        for name in ["AdminCopyOwner", "OwnerSid", "AdminCopyVersion", "ownersid"] {
            for value in [Value::Unreadable, Value::Text("")] {
                // The key's default value enumerates first, with an empty name; the walk goes on.
                let record = vec![
                    ("", Value::Unreadable),
                    text("AdminCopySha256", SHA),
                    (name, value),
                ];
                assert_eq!(check(owner, Some(record)), (3, false), "{owner} {name}");
            }
        }
        // Refused even beside a value naming this owner: the record is read whole first.
        let record = vec![text("AdminCopyOwner", ME), ("OwnerSid", Value::Unreadable)];
        assert_eq!(check(owner, Some(record)), (3, false), "{owner}");
    }
}

/// The arms of the `${If}` chain in `nodes`, at any depth, whose first condition is `first`.
fn chain<'a>(nodes: &'a [Node], first: &str) -> Option<&'a Node> {
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

fn lines(nodes: &[Node]) -> Vec<&str> {
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

#[test]
fn both_callers_write_the_copy_only_when_the_check_gives_0() {
    let code = admin_code();
    for (name, refused) in [
        (
            "CLAUSEO_ELEVATED_BRANCH",
            vec!["IntOp $R8 ${CLAUSEO_ADMIN_EXIT_BASE} + ${CLAUSEO_ADOPT_FAILED}"],
        ),
        (
            "CLAUSEO_ADMIN_COPY_MACHINE",
            vec![
                "StrCpy $R8 ${CLAUSEO_ADOPT_FAILED}",
                "!insertmacro CLAUSEO_ADMIN_COPY_MACHINE_SAYS",
            ],
        ),
    ] {
        let body = &macro_body(&code, name)[1..];
        let nodes = parse(body, &mut 0, &[]);
        let Some(Node::If(arms, Some(otherwise))) = chain(&nodes, "If $R1 = 1") else {
            panic!("{name}: no ${{If}} $R1 = 1 chain with an ${{Else}}");
        };
        let conds: Vec<&[String]> = arms.iter().map(|(conds, _)| conds.as_slice()).collect();
        assert_eq!(
            conds,
            [["If $R1 = 1"], ["If $R1 = 2"], ["If $R1 = 3"]],
            "{name}"
        );
        // Nothing written in a refusal: no File, no copy.
        for (_, arm) in arms {
            assert!(
                lines(arm)
                    .iter()
                    .all(|line| !line.contains("CLAUSEO_ADMIN_COPY ")
                        && !line.contains("CLAUSEO_ADMIN_COPY_BOUNDED")
                        && !line.starts_with("File ")),
                "{name}"
            );
        }
        assert_eq!(lines(&arms[2].1), refused, "{name}");
        assert!(
            lines(otherwise)
                .iter()
                .any(|line| line.starts_with("!insertmacro CLAUSEO_ADMIN_COPY")),
            "{name}"
        );
    }
    // The outer per-user instance reads 100 + 3 as --adopt's failure: adminCopyFailed.
    let ask = macro_body(&code, ASK);
    assert_run(
        ask,
        &[
            "${Case4} ${CLAUSEO_ADOPT_USAGE} ${CLAUSEO_ADOPT_FAILED} ${CLAUSEO_ADOPT_LOCK_HELD} \
             ${CLAUSEO_ADOPT_DATA_DIR}",
            "StrCpy $R6 1",
        ],
    );
    assert_run(
        ask,
        &["${ElseIf} $R6 = 1", "DetailPrint \"$(adminCopyFailed)\""],
    );
}
