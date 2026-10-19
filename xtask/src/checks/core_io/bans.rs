//! What the core-io check bans: paths inside `std`, matched segment by segment. This file is the
//! one list of them; the docs, the exception file and the Crates table point here.

use super::tokens;

/// What a pattern bans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A module: every path inside it and the module itself. Banned for the rows that list
    /// `fs`, `process` or `any I/O`.
    Module,
    /// A clock's `now` function (the pattern ends `<clock>::now`). Banned for every held row,
    /// also one that lists only `clock`.
    Clock,
}

/// One banned path, `*` matching any one segment. A path is banned when it starts with one.
struct Pattern {
    path: &'static [&'static str],
    kind: Kind,
}

const PATTERNS: [Pattern; 10] = [
    module(&["std", "fs"]),
    module(&["std", "net"]),
    module(&["std", "process"]),
    module(&["std", "env"]),
    module(&["std", "os", "*", "fs"]),
    module(&["std", "os", "*", "net"]),
    module(&["std", "os", "*", "process"]),
    // Re-exports the platform `fs`, `process` and `net` extension traits.
    module(&["std", "os", "*", "prelude"]),
    clock(&["std", "time", "Instant", "now"]),
    clock(&["std", "time", "SystemTime", "now"]),
];

const fn module(path: &'static [&'static str]) -> Pattern {
    Pattern {
        path,
        kind: Kind::Module,
    }
}

const fn clock(path: &'static [&'static str]) -> Pattern {
    Pattern {
        path,
        kind: Kind::Clock,
    }
}

/// Each clock pattern, split into its type's path (`std::time::Instant`) and its `now`.
fn clocks() -> impl Iterator<Item = (&'static [&'static str], &'static str)> {
    PATTERNS
        .iter()
        .filter(|pattern| pattern.kind == Kind::Clock)
        .filter_map(|pattern| {
            let (now, ty) = pattern.path.split_last()?;
            Some((ty, *now))
        })
}

/// True when every segment of `path` matches the pattern segment at its index.
fn matches(pattern: &[&str], path: &[String]) -> bool {
    pattern
        .iter()
        .zip(path)
        .all(|(part, seg)| *part == "*" || part == seg)
}

/// The patterns a crate's row bans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Bans {
    /// The module patterns (`fs`, `process` or `any I/O` in the row); the clocks are always
    /// banned.
    pub modules: bool,
}

impl Bans {
    fn patterns(self) -> impl Iterator<Item = &'static Pattern> {
        PATTERNS
            .iter()
            .filter(move |pattern| self.modules || pattern.kind == Kind::Clock)
    }

    /// The banned path `path` (a path inside `std`) is in or is, if any: `std::fs` for
    /// `std::fs::read`, `std::os::unix::fs` for `std::os::unix::fs::symlink`.
    pub(super) fn banned(self, path: &[String]) -> Option<String> {
        self.patterns().find_map(|pattern| {
            let head = path.get(..pattern.path.len())?;
            matches(pattern.path, head).then(|| tokens::join(head))
        })
    }

    /// The banned paths a glob import of `prefix`, or a path that a macro metavariable
    /// continues, may reach: the one it sits inside, else every banned path that starts with
    /// `prefix` (a `*` filled in from `prefix` where it can be).
    pub(super) fn reaches(self, prefix: &[String]) -> Vec<String> {
        if let Some(inside) = self.banned(prefix) {
            return vec![inside];
        }
        self.patterns()
            .filter(|pattern| pattern.path.len() > prefix.len() && matches(pattern.path, prefix))
            .map(|pattern| filled(pattern.path, prefix))
            .collect()
    }

    /// The banned paths `std::<path>` may reach when `path` has a metavariable in it (see
    /// `tokens::is_placeholder`): those of `std::<the segments before it>`. `None` when no
    /// segment is a metavariable.
    pub(super) fn std_metavariable(self, path: &[String]) -> Option<Vec<String>> {
        let at = path.iter().position(|seg| tokens::is_placeholder(seg))?;
        Some(self.reaches(path.get(..at).unwrap_or_default()))
    }

    /// The banned paths `$x::<rest>` may reach when the metavariable `$x` expands to a path:
    /// those `std::<rest>` reaches, and a clock's `now` when `rest` is `now` or
    /// `<clock>::now`. Nothing for an empty `rest`: `$x` alone names no banned path.
    pub(super) fn metavariable_reaches(self, rest: &[String]) -> Vec<String> {
        let mut in_std = vec!["std".to_owned()];
        in_std.extend(rest.iter().cloned());
        let mut found: Vec<String> = match self.std_metavariable(&in_std) {
            Some(reaches) => reaches,
            None => self.banned(&in_std).into_iter().collect(),
        };
        for (ty, now) in clocks() {
            let name = ty.last().copied().unwrap_or_default();
            let hit = match rest {
                [first, ..] if first == now => true,
                [clock, second, ..] => clock == name && second == now,
                _ => false,
            };
            let path = format!("{}::{now}", ty.join("::"));
            if hit && !found.contains(&path) {
                found.push(path);
            }
        }
        found
    }
}

/// `pattern` with each segment that `prefix` has at its index taken from `prefix`.
fn filled(pattern: &[&str], prefix: &[String]) -> String {
    let filled: Vec<String> = pattern
        .iter()
        .enumerate()
        .map(|(index, part)| {
            prefix
                .get(index)
                .map_or_else(|| (*part).to_owned(), Clone::clone)
        })
        .collect();
    tokens::join(&filled)
}
