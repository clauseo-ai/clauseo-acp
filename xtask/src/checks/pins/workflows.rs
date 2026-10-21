//! The workflow rule: no workflow in `.github/workflows/` and no action in `.github/actions/`
//! pins pnpm, Node, Rust or a tool `xtask/src/setup.rs` pins. It reads each `.yml` and `.yaml`
//! file at the top of `.github/workflows/` (in any case), and each `action.yml` and
//! `action.yaml` anywhere under `.github/actions/`, the composite actions a workflow runs with
//! `uses: ./.github/actions/<name>`. Each is parsed with `saphyr-parser`, so quoted keys, flow
//! mappings, anchors, aliases and merge keys read as YAML defines them; a composite action's
//! `runs.steps` are steps like a job's.
//!
//! A step (any mapping with `uses`) fails on:
//! - a `version` input to `pnpm/action-setup`, a `node-version` input to `actions/setup-node`, or
//!   the toolchain input of a Rust toolchain action (`TOOLCHAIN_ACTIONS`), whatever its value.
//!   Input names match in any case, as the runner passes them as `INPUT_<NAME>`;
//! - `dtolnay/rust-toolchain` at a version ref (`@1.2.3`), or at a commit whose comment names a
//!   version: its branches are named after the toolchain they install. At a commit with no
//!   comment, or one that names no version, the toolchain it installs cannot be told from the
//!   workflow, so that ref is not checked.
//!
//! Any other value (a `run` script, an `env` value, another action's input, a container or a
//! `docker://` image) fails where it names a pinned tool and a version, in the forms `forms.rs`
//! lists, and so does a `RUSTUP_TOOLCHAIN` key (any case) whose value is a version. Any version
//! there fails, the home's or another. Other keys, other numbers and comments are not read.

use super::forms::{self, Named};
use super::yaml::{self, Node, Value};
use super::{FileHome, Fix, Home, is_version};
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
/// Where the repo's composite actions are, each in an `action.yml` or `action.yaml`.
pub(super) const ACTIONS: &str = ".github/actions";

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

/// The environment variable that overrides the toolchain `rust-toolchain.toml` names.
const RUSTUP_TOOLCHAIN: &str = "RUSTUP_TOOLCHAIN";

pub(super) fn check(root: &Path, tools: &[Tool], binaries: &[Binary], found: &mut Inspection) {
    let mut paths = Vec::new();
    for (dir, list) in [
        (DIR, workflow_files as fn(&Path) -> io::Result<Vec<String>>),
        (ACTIONS, action_files),
    ] {
        match list(root) {
            Ok(files) => paths.extend(files),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => found.violations.push(checks::unreadable(dir, &error)),
        }
    }
    for path in paths {
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
            tools,
            binaries,
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

/// The paths of the `.yml` and `.yaml` files in `DIR`, sorted.
fn workflow_files(root: &Path) -> io::Result<Vec<String>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root.join(DIR))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_yaml(&name) && entry.file_type()?.is_file() {
            paths.push(format!("{DIR}/{name}"));
        }
    }
    paths.sort();
    Ok(paths)
}

/// The paths of the `action.yml` and `action.yaml` files anywhere under `ACTIONS`, sorted.
fn action_files(root: &Path) -> io::Result<Vec<String>> {
    let mut paths = Vec::new();
    let mut dirs = vec![ACTIONS.to_owned()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(root.join(&dir))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = entry.file_type()?;
            let stem = Path::new(&name)
                .file_stem()
                .is_some_and(|stem| stem == "action");
            if kind.is_dir() {
                dirs.push(format!("{dir}/{name}"));
            } else if kind.is_file() && stem && is_yaml(&name) {
                paths.push(format!("{dir}/{name}"));
            }
        }
    }
    paths.sort();
    Ok(paths)
}

/// True for a name that ends in `.yml` or `.yaml`, in any case.
fn is_yaml(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
    })
}

/// One file's walk. Each node is visited once, through whichever alias reaches it.
struct Walk<'a> {
    path: &'a str,
    lines: Vec<&'a str>,
    comments: HashMap<u32, &'a str>,
    tools: &'a [Tool],
    binaries: &'a [Binary],
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
                    self.rustup_toolchain(key, value);
                    self.node(value);
                }
            }
        }
    }

    /// A `RUSTUP_TOOLCHAIN` key whose value is a version.
    fn rustup_toolchain(&mut self, key: &Node, value: &Rc<Node>) {
        let Some(name) = key
            .scalar()
            .filter(|name| name.eq_ignore_ascii_case(RUSTUP_TOOLCHAIN))
        else {
            return;
        };
        let Some(version) = value.scalar().filter(|text| forms::is_named_version(text)) else {
            return;
        };
        // Its value is reported here, not read again.
        self.seen.insert(Rc::as_ptr(value));
        let named = Named {
            shown: format!("{name}: {version}"),
            version: version.to_owned(),
            home: Home::File(FileHome::Rust),
        };
        self.copy(value, &named);
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
            .map(|(_, input)| (*input, FileHome::Rust));
        let input = match action.as_str() {
            "pnpm/action-setup" => Some(("version", FileHome::Pnpm)),
            "actions/setup-node" => Some(("node-version", FileHome::Node)),
            _ => toolchain,
        };
        if let (Some((input, home)), Some(with)) = (input, field("with")) {
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
                    home.tool(),
                    home.place()
                );
                self.report(key.line, message, Fix::Input(home));
            }
        }
        if action == DTOLNAY {
            self.dtolnay(uses, text, reference);
        }
    }

    /// `dtolnay/rust-toolchain` at a version ref, or at a commit whose comment names one.
    fn dtolnay(&mut self, uses: &Node, text: &str, reference: &str) {
        let home = FileHome::Rust.place();
        if is_version(reference) {
            let message = format!(
                "`uses: {text}` tells {DTOLNAY} which Rust toolchain to install through its ref; \
                 the one home of that version is {home}"
            );
            self.report(uses.line, message, Fix::Input(FileHome::Rust));
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
            self.report(uses.line, message, Fix::Input(FileHome::Rust));
        }
    }

    /// A value that names a pinned tool and a version.
    fn scalar(&mut self, node: &Node, text: &str) {
        for named in forms::named(text, self.tools, self.binaries) {
            self.copy(node, &named);
        }
    }

    /// Reports `named`, at the line of `node` that holds its version.
    fn copy(&mut self, node: &Node, named: &Named) {
        let first = usize::try_from(node.line.saturating_sub(1)).unwrap_or(usize::MAX);
        let last = usize::try_from(node.end_line).unwrap_or(usize::MAX);
        let line = (first..last.max(first.saturating_add(1)))
            .find(|index| {
                self.lines
                    .get(*index)
                    .is_some_and(|line| holds(line, &named.version))
            })
            .map_or(node.line, checks::line_at);
        let message = format!(
            "`{}` names {} {}; the one home of {} is {}",
            named.shown,
            named.home.tool(),
            named.version,
            named.home.noun(),
            named.home.place()
        );
        self.report(line, message, Fix::Copy(named.home));
    }

    fn report(&mut self, line: u32, message: String, fix: Fix) {
        self.found
            .push(checks::violation(self.path, line, message, fix));
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
