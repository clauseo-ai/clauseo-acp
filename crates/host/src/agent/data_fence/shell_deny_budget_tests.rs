//! The auto-mode context a Windows Claude launch's deny rules cost. Claude Code 2.1.293 sends
//! the context (its permission rules among it) with each call only while it is at most
//! 1,040,000 bytes; past that every Edit and Bash call gets "no server verdict for this call"
//! for the whole launch. Measured against the pinned binary through a stand-in for the Messages
//! API (`scripts/claude-rule-probe`): each rule is `{"rule":…,"source":…}`, its JSON plus 33 B;
//! a `Read` or `Edit` rule also carries its root and pattern, about 225 B in all; allow rules
//! count nothing in auto mode; about 3 KB is fixed.
//!
//! The launch here is the laptop's (`big_folder::laptop`) with 1, 8, 50 and 200 registered
//! matters (the open one and N − 1 others at `Documents\Matters\Seeded Matter NN`), the data
//! folder spelled the default way and, as the launch adds it where the profile has one, by its
//! 8.3 short form too (the other matters and the own one then go by theirs as well), and the
//! variant where one other matter's folder is a text prefix of the open matter's
//! (`Documents\Launch` beside `Documents\Launch Matter`). The rules are built as
//! `loads::profile::deny_data_folder` builds them on Windows: Read and Edit (the data folder's,
//! the other matters', the folders above them), the shell rules with their variable forms, the
//! matter marker; the allow rules are built and counted as nothing. The per-conversation groups
//! (`run`, `agent-state`, Claude's projects) are the laptop's own and are not varied here.

use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_protocol::MatterId;

use super::big_folder::laptop;
use super::{LaunchDeny, claude_allow_rules_on, claude_deny_rules_on, windows_shell_deny_rules};
use crate::agent::matter_fence::{MatterFence, Other};
use crate::pipeline::tests::support::TempDir;

const HOME: &str = r"C:\Users\clauseo-lawyer";
const SHORT_HOME: &str = r"C:\Users\CLAUSE~1";
const DATA: &str = r"AppData\Local\ai.clauseo.desktop";
const SHORT_DATA: &str = r"AppData\Local\AICLAU~1.DES";

/// The most context a launch may cost, by its matters, with one data spelling or two (two is the
/// ordinary Windows machine: `ai.clauseo.desktop` is never an 8.3 name, so a volume that makes
/// short names gives it `AICLAU~1.DES` too): an ordinary lawyer is never sent to ask mode, whose
/// first-party line is 900,000 B (87% of Claude Code's 1,040,000 B). 810,000 B at 200 matters is
/// that line less 10%. Decision record 0030, "Auto-mode context". A row over its bound is a real
/// finding: cut rules, never raise the bound.
const BOUNDS: [(usize, usize); 3] = [(8, 600_000), (50, 700_000), (200, 810_000)];

/// The context a launch's rules cost: each deny rule's JSON (its backslashes doubled) plus 33 B,
/// or about 225 B for a `Read` or `Edit` rule (root and pattern too); 3 KB fixed. Allow rules
/// cost nothing in auto mode, so they are not passed.
// TODO: replace with `agent::auto_context::estimate` once the ask-mode PR lands it.
fn estimate(deny: &[String]) -> usize {
    let rule = |rule: &String| {
        if rule.starts_with("Read(") || rule.starts_with("Edit(") {
            225
        } else {
            serde_json::to_string(rule).map_or(0, |json| json.len()) + 33
        }
    };
    3_000 + deny.iter().map(rule).sum::<usize>()
}

fn env(name: &str) -> Option<OsString> {
    match name {
        "LOCALAPPDATA" => Some(format!(r"{HOME}\AppData\Local").into()),
        "APPDATA" => Some(format!(r"{HOME}\AppData\Roaming").into()),
        "USERPROFILE" | "HOME" => Some(HOME.into()),
        _ => None,
    }
}

fn id(n: usize) -> MatterId {
    let n = u128::try_from(n).unwrap();
    let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
    MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
}

/// What a launch was built from, and its rules.
struct Launch {
    deny: Vec<String>,
    allow: Vec<String>,
    ancestors: usize,
    /// The own folders' Windows rules (`own_shell_allow_rules`, `own_shell_glob_deny_rules`):
    /// fixed per launch, in the settings file before any other matter's rule.
    own_folder: usize,
    /// Of those, the glob denies' context (the allow rules' is nothing).
    glob_context: usize,
}

/// The laptop's launch with `matters` registered matters, the 8.3 spellings too when `short`,
/// the first other matter at `Documents\Launch` when `prefix`.
fn launch(matters: usize, short: bool, prefix: bool) -> Launch {
    let dir = TempDir::new(&format!("budget-{matters}-{short}-{prefix}"));
    let (deny, _, _) = laptop(dir.path(), 1, matters - 1, "clauseo-lawyer");
    let mut spellings = vec![PathBuf::from(format!(r"{HOME}\{DATA}"))];
    if short {
        spellings.push(PathBuf::from(format!(r"{SHORT_HOME}\{SHORT_DATA}")));
    }
    let both = |long: String, short_form: String| {
        let mut out = vec![PathBuf::from(long)];
        if short {
            out.push(PathBuf::from(short_form));
        }
        out
    };
    let others = (1..matters)
        .map(|n| {
            let (long, short_form) = if prefix && n == 1 {
                (
                    format!(r"{HOME}\Documents\Launch"),
                    format!(r"{SHORT_HOME}\Documents\Launch"),
                )
            } else {
                (
                    format!(r"{HOME}\Documents\Matters\Seeded Matter {n:02}"),
                    format!(r"{SHORT_HOME}\Documents\Matters\SEEDED~{n}"),
                )
            };
            let mut other = Other::new(id(n), format!("Matter {n}"), long.clone().into(), false);
            other.spellings = both(long, short_form);
            other.paths = other.spellings.clone();
            other
        })
        .collect();
    let mut fence = MatterFence::asked(others);
    fence.own = both(
        format!(r"{HOME}\Documents\Launch Matter"),
        format!(r"{SHORT_HOME}\Documents\LAUNCH~1"),
    );
    let deny = LaunchDeny {
        spellings,
        matters: fence,
        ..deny
    };
    let mut rules = claude_deny_rules_on(&deny, true, false);
    let ancestors = rules
        .iter()
        .filter(|rule| rule.starts_with("Edit(") && !rule.ends_with("/**)"))
        .filter(|rule| rule.contains("/Documents/Matters)") || rule.contains("/Documents)"))
        .count();
    let mut known: std::collections::HashSet<String> = rules.iter().cloned().collect();
    for rule in windows_shell_deny_rules(&deny, &env) {
        if known.insert(rule.clone()) {
            rules.push(rule);
        }
    }
    let marker = super::rule_path(&format!(r"{HOME}\Documents\Launch Matter"), true);
    let marker_name = crate::agent::loads::sandbox::MATTER_MARKER;
    rules.push(format!("Edit({marker}/{marker_name})"));
    rules.push(format!("Edit({marker}/{marker_name}/**)"));
    let glob = super::own_shell_glob_deny_rules(&deny, true);
    let own_folder = super::own_shell_allow_rules(&deny, true).len() + glob.len();
    let glob_context = estimate(&glob) - 3_000;
    Launch {
        deny: rules,
        allow: claude_allow_rules_on(&deny, true),
        ancestors,
        own_folder,
        glob_context,
    }
}

/// The context at 1, 8, 50 and 200 matters, with one data spelling and with two, and with a
/// matter whose folder is a text prefix of the open one's: under [`BOUNDS`] at 8, 50 and 200
/// matters, whichever way (1 matter is printed, not bounded). Every row is printed for the PR
/// (`--no-capture`) before any is checked: context, settings bytes, deny and allow counts.
#[test]
fn a_windows_launch_stays_within_the_auto_mode_context() {
    let mut seen = Vec::new();
    for short in [false, true] {
        for prefix in [false, true] {
            for matters in [1, 8, 50, 200] {
                let launch = launch(matters, short, prefix);
                let context = estimate(&launch.deny);
                // The settings file's share (`loads::profile::SHED_AT`), the own folders' rules
                // in it first.
                let file = serde_json::to_string(&launch.deny).unwrap().len()
                    + serde_json::to_string(&launch.allow).unwrap().len();
                eprintln!(
                    "budget: {matters} matters, {} spelling(s){}: {} deny rules ({} above \
                     other matters), {} allow, {} own-folder rules (glob denies {} B of context); \
                     context {context} B; settings {file} B",
                    if short { 2 } else { 1 },
                    if prefix { ", prefix matter" } else { "" },
                    launch.deny.len(),
                    launch.ancestors,
                    launch.allow.len(),
                    launch.own_folder,
                    launch.glob_context,
                );
                assert!(
                    file < crate::agent::loads::profile::SHED_AT,
                    "{matters}: {file}"
                );
                seen.push((matters, short, prefix, context));
            }
        }
    }
    // Every row printed first, then checked.
    for (matters, short, prefix, context) in &seen {
        let Some((_, bound)) = BOUNDS.iter().find(|(at, _)| at == matters) else {
            continue;
        };
        assert!(
            context < bound,
            "{matters} matters, short {short}, prefix {prefix}: {context} >= {bound}; {seen:?}"
        );
    }
    // More matters never cost less.
    for row in seen.chunks(4) {
        assert!(
            row.windows(2).all(|pair| pair[0].3 <= pair[1].3),
            "{seen:?}"
        );
    }
}

/// The estimate against what the pinned binary sent through the stand-in API for these launches
/// at #1085's b0d68aad7 with no other matters' shell rules and the 8.3 data spelling written as
/// its name alone (`autoctx`'s variant V4; one and two spellings): never under it, and within
/// 10% of it, the folders above other matters (new since) left out. The rest of the gap is the
/// deny rules the head gained by its rebase on main 80ca79ecf (192 a data spelling at one
/// matter) and the estimate's flat 225 B a Read or Edit rule.
#[test]
fn the_estimate_is_pinned_to_the_measured_context() {
    let measured: [(usize, bool, usize); 8] = [
        (1, false, 364_151),
        (8, false, 377_425),
        (50, false, 423_195),
        (200, false, 539_671),
        (1, true, 496_475),
        (8, true, 519_047),
        (50, true, 600_389),
        (200, true, 815_781),
    ];
    let mut rows = Vec::new();
    for (matters, short, sent) in measured {
        let launch = launch(matters, short, false);
        let estimate = estimate(&launch.deny) - 225 * launch.ancestors;
        eprintln!(
            "pin: {matters} matters, short {short}: estimate {estimate} B, measured {sent} B"
        );
        rows.push((matters, short, estimate, sent));
    }
    for (matters, short, estimate, sent) in rows {
        assert!(estimate >= sent, "{matters} {short}: {estimate} < {sent}");
        assert!(
            estimate <= sent + sent / 10,
            "{matters} {short}: {estimate} vs {sent}"
        );
    }
}
