//! Where a workflow value names a pinned tool, and the version it names there. A version counts
//! only in these forms, so a number that belongs to anything else (`python-version: '3.4'`) is
//! not read:
//! - `<name>@<v>`, for `pnpm`, `node` and each name in `TOOLS` and `BINARIES`: `npm i -g
//!   pnpm@<v>`, `corepack prepare pnpm@<v>`, `cargo install <tool>@<v>`, `tool: <tool>@<v>`;
//! - `node:<v>` and `rust:<v>`, the container images, `docker://` and a registry path included;
//! - in a command: `rustup toolchain install|add <v>...`, `rustup install|update <v>...`,
//!   `rustup default|run <v>`, `rustup override set <v>`, `--toolchain <v>` in any rustup command,
//!   `+<v>` after `cargo`, `rustc` or `rustdoc`, `fnm install|use <v>`, `nvm install|use <v>`,
//!   and `cargo install|binstall <tool> ... --version <v>` (or `--vers`) for a tool in `TOOLS` or
//!   `BINARIES`.
//!
//! A version is a word that starts with a digit after any of `v^~=<>`: `7`, `7.8.9`, `v7`, `>=7`,
//! `6-alpine`. A channel or a tag (`stable`, `lts/*`, `latest`) is no version. Names match in any
//! case. A command is a line, or the part of one between `;`, `|` and `&`, with `\` and a newline
//! read as a space; its words are split on whitespace, with quotes and brackets at their ends
//! dropped. A command split some other way (a variable for the version, `eval`) is not read.

use super::{FileHome, Home};
use crate::setup::{Binary, Tool};

/// A version named in a value, as `forms` finds it.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Named {
    /// The words that name it, as written: `pnpm@7`, `rustup toolchain install 1.2.3`.
    pub shown: String,
    /// The version: `7`, `1.2.3`.
    pub version: String,
    /// The home of the tool it names.
    pub home: Home,
}

/// Each version `text` names, in the order of the forms above.
pub(super) fn named(text: &str, tools: &[Tool], binaries: &[Binary]) -> Vec<Named> {
    let mut names = vec![
        ("pnpm", Home::File(FileHome::Pnpm)),
        ("node", Home::File(FileHome::Node)),
    ];
    names.extend(tools.iter().map(|tool| (tool.name, Home::Tool(tool.name))));
    names.extend(binaries.iter().map(|b| (b.name, Home::Binary(b.name))));
    let mut out = Vec::new();
    for (name, home) in &names {
        after_name(text, name, '@', *home, &mut out);
    }
    after_name(text, "node", ':', Home::File(FileHome::Node), &mut out);
    after_name(text, "rust", ':', Home::File(FileHome::Rust), &mut out);
    let joined = text.replace("\\\n", " ");
    for command in joined.split(['\n', ';', '|', '&']) {
        let words: Vec<&str> = command
            .split_whitespace()
            .map(|word| word.trim_matches(['"', '\'', '`', '(', ')', '[', ']', '{', '}']))
            .collect();
        for at in 0..words.len() {
            in_command(&words, at, &names, &mut out);
        }
    }
    out
}

/// True for a word that is a version: a digit first, after any of `v^~=<>`.
pub(super) fn is_named_version(word: &str) -> bool {
    word.trim_start_matches(['v', 'V', '^', '~', '=', '<', '>'])
        .starts_with(|ch: char| ch.is_ascii_digit())
}

/// `name` and `sep` then a version, with no part of a longer name just before `name`.
fn after_name(text: &str, name: &str, sep: char, home: Home, out: &mut Vec<Named>) {
    let lower = text.to_ascii_lowercase();
    for (at, _) in lower.match_indices(&name.to_ascii_lowercase()) {
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        if before.is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')) {
            continue;
        }
        let rest = text
            .get(at.saturating_add(name.len())..)
            .unwrap_or_default();
        let Some(rest) = rest.strip_prefix(sep) else {
            continue;
        };
        let end = rest
            .find(|ch: char| ch.is_whitespace() || "\"'`,;()[]{}|&".contains(ch))
            .unwrap_or(rest.len());
        let version = rest.get(..end).unwrap_or_default();
        if is_named_version(version) {
            let written = text.get(at..at.saturating_add(name.len())).unwrap_or(name);
            out.push(Named {
                shown: format!("{written}{sep}{version}"),
                version: version.to_owned(),
                home,
            });
        }
    }
}

/// The command forms whose program is `words[at]`.
fn in_command(words: &[&str], at: usize, names: &[(&str, Home)], out: &mut Vec<Named>) {
    let Some(program) = words.get(at) else {
        return;
    };
    let program = program
        .rsplit('/')
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    let args = words.get(at.saturating_add(1)..).unwrap_or_default();
    let mut push = |last: usize, version: &str, home: Home| {
        let shown = words.get(at..=at.saturating_add(1).saturating_add(last));
        out.push(Named {
            shown: shown.map(|words| words.join(" ")).unwrap_or_default(),
            version: version.to_owned(),
            home,
        });
    };
    let rust = Home::File(FileHome::Rust);
    match program.as_str() {
        "rustup" => {
            for (index, version) in rustup(args) {
                push(index, version, rust);
            }
        }
        "cargo" | "rustc" | "rustdoc" => {
            if let Some(version) = args.first().and_then(|word| word.strip_prefix('+'))
                && is_named_version(version)
            {
                push(0, version, rust);
            }
            if program == "cargo"
                && let Some((index, version, home)) = cargo_install(args, names)
            {
                push(index, version, home);
            }
        }
        "fnm" | "nvm" => {
            let mut positional = args.iter().enumerate().filter(|(_, w)| !w.starts_with('-'));
            let verb = positional.next().map(|(_, word)| *word);
            if let (Some("install" | "use"), Some((index, version))) = (verb, positional.next())
                && is_named_version(version)
            {
                push(index, version, Home::File(FileHome::Node));
            }
        }
        _ => {}
    }
}

/// The toolchains a rustup command names, each with its index in `args`.
fn rustup<'w>(args: &[&'w str]) -> Vec<(usize, &'w str)> {
    let mut out = Vec::new();
    let mut positional = Vec::new();
    let mut words = args.iter().copied().enumerate();
    while let Some((index, word)) = words.next() {
        if word == "--toolchain" {
            if let Some((index, value)) = words.next() {
                out.push((index, value));
            }
        } else if let Some(value) = word.strip_prefix("--toolchain=") {
            out.push((index, value));
        } else if !word.starts_with('-') {
            positional.push((index, word));
        }
    }
    let verbs: Vec<&str> = positional.iter().map(|(_, word)| *word).collect();
    let (skip, all) = match verbs.as_slice() {
        ["toolchain", "install" | "add", ..] => (2, true),
        ["install" | "update", ..] => (1, true),
        ["default" | "run", ..] => (1, false),
        ["override", "set", ..] => (2, false),
        _ => (positional.len(), false),
    };
    let named = positional.into_iter().skip(skip);
    out.extend(named.take(if all { usize::MAX } else { 1 }));
    out.retain(|(_, word)| is_named_version(word));
    out
}

/// `cargo install|binstall <tool> --version <v>` for a pinned tool: the index in `args` of the
/// later of the two words, the version and the tool's home.
fn cargo_install<'w>(args: &[&'w str], names: &[(&str, Home)]) -> Option<(usize, &'w str, Home)> {
    let mut words = args.iter().copied().enumerate();
    let verb = words
        .by_ref()
        .find(|(_, word)| !word.starts_with('-') && !word.starts_with('+'))?;
    if !matches!(verb.1, "install" | "binstall") {
        return None;
    }
    let (mut version, mut tool) = (None, None);
    while let Some((index, word)) = words.next() {
        if word == "--version" || word == "--vers" {
            version = words.next().or(version);
        } else if let Some(value) = word
            .strip_prefix("--version=")
            .or_else(|| word.strip_prefix("--vers="))
        {
            version = Some((index, value));
        } else if !word.starts_with('-') {
            let crate_name = word.split('@').next().unwrap_or(word);
            let home = names
                .iter()
                .filter(|(_, home)| matches!(home, Home::Tool(_) | Home::Binary(_)))
                .find(|(name, _)| name.eq_ignore_ascii_case(crate_name))
                .map(|(_, home)| *home);
            if let (None, Some(home)) = (tool, home) {
                tool = Some((index, home));
            }
        }
    }
    let ((at_version, version), (at_tool, home)) = (version?, tool?);
    is_named_version(version).then_some((at_version.max(at_tool), version, home))
}
