//! The workflow rule: no file in `.github/workflows/` pins pnpm, Node or Rust, or repeats a
//! version a home pins. Each `.yml` and `.yaml` file there (in any case) is parsed with
//! `saphyr-parser`, so quoted keys, flow mappings, anchors, aliases and merge keys read as YAML
//! defines them.
//!
//! A step (any mapping with `uses`) fails on:
//! - a `version` input to `pnpm/action-setup`, a `node-version` input to `actions/setup-node`, or
//!   the toolchain input of a Rust toolchain action (`TOOLCHAIN_ACTIONS`). Input names match in
//!   any case, as the runner passes them as `INPUT_<NAME>`;
//! - `dtolnay/rust-toolchain` at a version ref (`@1.2.3`), or at a commit whose comment names a
//!   version: its branches are named after the toolchain they install.
//!
//! Any other value (a `run` script, an `env` value, another action's input, a `docker://` image)
//! fails when it holds a version a home pins, or the major.minor of the Rust, pnpm or Node one,
//! as a whole word: `1.2.3` in `pnpm@1.2.3`, not in `11.2.3`. Keys and comments are not read.

use super::homes::Homes;
use super::yaml::{self, Node, Value};
use super::{Fix, Home, PACKAGE_JSON, TOOLCHAIN, is_version, major_minor};
use crate::checks::{self, Inspection};
use crate::setup::{Binary, Tool};
use crate::violation::Violation;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;
use std::rc::Rc;

/// Where GitHub reads workflows from: only the files at its top level.
pub(super) const DIR: &str = ".github/workflows";

/// Rust toolchain actions and the input that names the toolchain, from each action's
/// `action.yml`.
const TOOLCHAIN_ACTIONS: &[(&str, &str)] = &[
    ("dtolnay/rust-toolchain", "toolchain"),
    ("actions-rust-lang/setup-rust-toolchain", "toolchain"),
    ("actions-rs/toolchain", "toolchain"),
    ("moonrepo/setup-rust", "channel"),
];

/// The action whose ref names the toolchain it installs.
const DTOLNAY: &str = "dtolnay/rust-toolchain";

pub(super) fn check(
    root: &Path,
    homes: &Homes,
    tools: &[Tool],
    binaries: &[Binary],
    found: &mut Inspection,
) {
    let files = match workflow_files(root) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            found.violations.push(checks::unreadable(DIR, &error));
            return;
        }
    };
    let copies = copies(homes, tools, binaries);
    for name in files {
        let path = format!("{DIR}/{name}");
        found.inspected = found.inspected.saturating_add(1);
        let text = match fs::read_to_string(root.join(&path)) {
            Ok(text) => text,
            Err(error) => {
                found.violations.push(checks::unreadable(&path, &error));
                continue;
            }
        };
        let doc = match yaml::parse(&text) {
            Ok(doc) => doc,
            Err(error) => {
                found.violations.push(checks::violation(
                    &path,
                    error.line,
                    format!("{path} is not YAML: {}", error.info),
                    Fix::NotYaml,
                ));
                continue;
            }
        };
        let mut walk = Walk {
            path: &path,
            lines: text.lines().collect(),
            comments: doc.comments(&text).into_iter().collect(),
            copies: &copies,
            seen: HashSet::new(),
            found: Vec::new(),
        };
        for node in doc.roots() {
            walk.node(node);
        }
        for violation in walk.found {
            if !found.violations.contains(&violation) {
                found.violations.push(violation);
            }
        }
    }
}

/// The names of the `.yml` and `.yaml` files in `DIR`, sorted.
fn workflow_files(root: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root.join(DIR))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let yaml = Path::new(&name).extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
        });
        if yaml && entry.file_type()?.is_file() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// A version a workflow must not hold, and the home that pins it.
struct Pin {
    text: String,
    home: Home,
    /// True for the major.minor of the home's version.
    major_minor: bool,
}

/// Every version the homes pin: Rust, pnpm and Node with their major.minor, and each tool and
/// binary in `setup.rs`.
fn copies(homes: &Homes, tools: &[Tool], binaries: &[Binary]) -> Vec<Pin> {
    let mut out = Vec::new();
    let read = [
        (Home::Rust, &homes.rust),
        (Home::Pnpm, &homes.pnpm),
        (Home::Node, &homes.node),
    ];
    for (home, version) in read {
        let Some(version) = version else {
            continue;
        };
        out.push(Pin {
            text: version.clone(),
            home,
            major_minor: false,
        });
        if major_minor(version) != version {
            out.push(Pin {
                text: major_minor(version).to_owned(),
                home,
                major_minor: true,
            });
        }
    }
    let tools = tools
        .iter()
        .map(|tool| (tool.version, Home::Tool(tool.name)));
    let binaries = binaries
        .iter()
        .map(|binary| (binary.version, Home::Binary(binary.name)));
    for (version, home) in tools.chain(binaries) {
        out.push(Pin {
            text: version.to_owned(),
            home,
            major_minor: false,
        });
    }
    out
}

/// One workflow file's walk. Each node is visited once, through whichever alias reaches it.
struct Walk<'a> {
    path: &'a str,
    lines: Vec<&'a str>,
    comments: HashMap<u32, &'a str>,
    copies: &'a [Pin],
    seen: HashSet<*const Node>,
    found: Vec<Violation>,
}

impl Walk<'_> {
    fn node(&mut self, node: &Rc<Node>) {
        if !self.seen.insert(Rc::as_ptr(node)) {
            return;
        }
        match &node.value {
            Value::Scalar(text) => self.scalar(node, text),
            Value::Seq(items) => {
                for item in items {
                    self.node(item);
                }
            }
            Value::Map(_) => {
                let entries = node.entries();
                self.step(&entries);
                for (key, value) in &entries {
                    let uses = key.scalar() == Some("uses");
                    let image = value
                        .scalar()
                        .is_some_and(|text| text.starts_with("docker://"));
                    if uses && !image {
                        // An action's ref is its own version, not a copy of a pin.
                        self.seen.insert(Rc::as_ptr(value));
                    }
                    self.node(value);
                }
            }
        }
    }

    /// The rules for a step: its inputs and, for `dtolnay/rust-toolchain`, its ref.
    fn step(&mut self, entries: &[(Rc<Node>, Rc<Node>)]) {
        let field = |name: &str| {
            entries
                .iter()
                .find(|(key, _)| key.scalar() == Some(name))
                .map(|(_, value)| value)
        };
        let Some(uses) = field("uses") else {
            return;
        };
        let Some(text) = uses.scalar() else {
            return;
        };
        let (name, reference) = text.split_once('@').unwrap_or((text, ""));
        let action = name.to_ascii_lowercase();
        let toolchain = TOOLCHAIN_ACTIONS
            .iter()
            .find(|(known, _)| *known == action)
            .map(|(_, input)| (*input, Pinned::Rust));
        let input = match action.as_str() {
            "pnpm/action-setup" => Some(("version", Pinned::Pnpm)),
            "actions/setup-node" => Some(("node-version", Pinned::Node)),
            _ => toolchain,
        };
        if let (Some((input, pinned)), Some(with)) = (input, field("with")) {
            for (key, value) in with.entries() {
                let Some(name) = key.scalar().filter(|name| name.eq_ignore_ascii_case(input))
                else {
                    continue;
                };
                // Its value is reported here, not again as a copy.
                self.seen.insert(Rc::as_ptr(&value));
                let message = format!(
                    "`{name}` tells {action} which {} to install; the one home of that version \
                     is {}",
                    pinned.what(),
                    pinned.home()
                );
                self.report(key.line, message, pinned.fix());
            }
        }
        if action == DTOLNAY {
            self.dtolnay(uses, text, reference);
        }
    }

    /// `dtolnay/rust-toolchain` at a version ref, or at a commit whose comment names one.
    fn dtolnay(&mut self, uses: &Node, text: &str, reference: &str) {
        let home = Pinned::Rust.home();
        if is_version(reference) {
            let message = format!(
                "`uses: {text}` tells {DTOLNAY} which Rust toolchain to install through its ref; \
                 the one home of that version is {home}"
            );
            self.report(uses.line, message, Fix::RustToolchain);
            return;
        }
        let commit = reference.len() == 40 && reference.bytes().all(|b| b.is_ascii_hexdigit());
        let comment = self.comments.get(&uses.line).copied().unwrap_or_default();
        let named = comment.split_whitespace().find_map(|word| {
            let word = word.strip_prefix(['v', 'V']).unwrap_or(word);
            let word = word.trim_end_matches(|ch: char| !ch.is_ascii_digit());
            is_version(word).then_some(word)
        });
        if let (true, Some(named)) = (commit, named) {
            let message = format!(
                "`uses: {text}` pins {DTOLNAY} by commit, and the comment on its line names \
                 {named}, the Rust toolchain a commit of its `{named}` branch installs; the one \
                 home of that version is {home}"
            );
            self.report(uses.line, message, Fix::RustToolchain);
        }
    }

    /// A value that holds a version a home pins.
    fn scalar(&mut self, node: &Node, text: &str) {
        for copy in self.copies {
            if !holds(text, &copy.text) {
                continue;
            }
            let first = usize::try_from(node.line.saturating_sub(1)).unwrap_or(usize::MAX);
            let last = usize::try_from(node.end_line).unwrap_or(usize::MAX);
            let line = (first..last.max(first.saturating_add(1)))
                .find(|index| {
                    self.lines
                        .get(*index)
                        .is_some_and(|line| holds(line, &copy.text))
                })
                .map_or(node.line, checks::line_at);
            let what = if copy.major_minor {
                format!("the major.minor of {}", copy.home)
            } else {
                copy.home.to_string()
            };
            let message = format!("`{}` is {what}", copy.text);
            self.report(line, message, Fix::Copy(copy.home));
        }
    }

    fn report(&mut self, line: u32, message: String, fix: Fix) {
        self.found
            .push(checks::violation(self.path, line, message, fix));
    }
}

/// What a version input pins.
#[derive(Debug, Clone, Copy)]
enum Pinned {
    Pnpm,
    Node,
    Rust,
}

impl Pinned {
    fn what(self) -> &'static str {
        match self {
            Pinned::Pnpm => "pnpm",
            Pinned::Node => "Node",
            Pinned::Rust => "Rust toolchain",
        }
    }

    fn home(self) -> String {
        match self {
            Pinned::Pnpm => format!("`packageManager` in {PACKAGE_JSON}"),
            Pinned::Node => format!("`engines.node` in {PACKAGE_JSON}"),
            Pinned::Rust => format!("`channel` in {TOOLCHAIN}"),
        }
    }

    fn fix(self) -> Fix {
        match self {
            Pinned::Pnpm => Fix::PnpmInput,
            Pinned::Node => Fix::NodeInput,
            Pinned::Rust => Fix::RustToolchain,
        }
    }
}

/// True when `text` holds `version` as a whole word: no digit or `.` just before it, and no
/// digit, or `.` and a digit, just after it.
pub(super) fn holds(text: &str, version: &str) -> bool {
    text.match_indices(version).any(|(at, _)| {
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        let after = text
            .get(at.saturating_add(version.len())..)
            .unwrap_or_default();
        let mut next = after.chars();
        let clear_after = match next.next() {
            None => true,
            Some('.') => !next.next().is_some_and(|ch| ch.is_ascii_digit()),
            Some(ch) => !ch.is_ascii_digit(),
        };
        before.is_none_or(|ch| !ch.is_ascii_digit() && ch != '.') && clear_after
    })
}
