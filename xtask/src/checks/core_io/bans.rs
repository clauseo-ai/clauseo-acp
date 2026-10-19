//! What the core-io check bans: paths inside `std`, matched segment by segment.

/// The banned paths, `*` matching any one segment. A path is banned when it starts with one of
/// them; a module pattern bans every path inside the module and the module itself.
const PATTERNS: [&[&str]; 9] = [
    &["std", "fs"],
    &["std", "net"],
    &["std", "process"],
    &["std", "env"],
    &["std", "os", "*", "fs"],
    &["std", "os", "*", "net"],
    &["std", "os", "*", "process"],
    &["std", "time", "Instant", "now"],
    &["std", "time", "SystemTime", "now"],
];

/// The types whose `now` is banned.
pub(super) const CLOCKS: [&str; 2] = ["Instant", "SystemTime"];

/// Which of the patterns a crate's row bans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Bans {
    /// The modules (`fs`, `process` or `any I/O` in the row); the clocks are always banned.
    pub modules: bool,
}

impl Bans {
    fn patterns(self) -> impl Iterator<Item = &'static [&'static str]> {
        PATTERNS
            .into_iter()
            .filter(move |pattern| self.modules || pattern.get(1) == Some(&"time"))
    }

    /// The banned path `path` (a path inside `std`) is in or is, if any: `std::fs` for
    /// `std::fs::read`, `std::os::unix::fs` for `std::os::unix::fs::symlink`.
    pub(super) fn banned(self, path: &[String]) -> Option<String> {
        self.patterns().find_map(|pattern| {
            let head = path.get(..pattern.len())?;
            let matches = pattern
                .iter()
                .zip(head)
                .all(|(part, seg)| *part == "*" || part == seg);
            matches.then(|| head.join("::"))
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
            .filter(|pattern| {
                pattern.len() > prefix.len()
                    && pattern
                        .iter()
                        .zip(prefix)
                        .all(|(part, seg)| *part == "*" || part == seg)
            })
            .map(|pattern| {
                let filled: Vec<&str> = pattern
                    .iter()
                    .enumerate()
                    .map(|(index, part)| prefix.get(index).map_or(*part, String::as_str))
                    .collect();
                filled.join("::")
            })
            .collect()
    }

    /// The banned paths `$x::<rest>` may reach when the metavariable `$x` expands to a path:
    /// `std::<rest>` when that is banned, and a clock's `now` when `rest` is `now` or
    /// `<clock>::now`.
    pub(super) fn metavariable_reaches(self, rest: &[String]) -> Vec<String> {
        let mut found = Vec::new();
        let mut in_std = vec!["std".to_owned()];
        in_std.extend(rest.iter().cloned());
        found.extend(self.banned(&in_std));
        let clocks: Vec<&str> = match rest {
            [now, ..] if now == "now" => CLOCKS.to_vec(),
            [clock, now, ..] if now == "now" && CLOCKS.contains(&clock.as_str()) => {
                vec![clock.as_str()]
            }
            _ => Vec::new(),
        };
        found.extend(
            clocks
                .iter()
                .map(|clock| format!("std::time::{clock}::now")),
        );
        found
    }
}

/// The clock `path` names, if it is exactly `std::time::Instant` or `std::time::SystemTime`.
pub(super) fn clock(path: &[String]) -> Option<&'static str> {
    match path {
        [std, time, clock] if std == "std" && time == "time" => {
            CLOCKS.iter().copied().find(|known| known == clock)
        }
        _ => None,
    }
}
