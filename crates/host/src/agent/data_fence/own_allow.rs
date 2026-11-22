//! Claude's `Bash(…)` and `PowerShell(…)` allow rules for listing and reading the session's own
//! work and notes folders (decision record 0030, "The own folders' allow"). Claude runs in `auto`
//! mode, which on Windows refused `ls` and `Get-ChildItem -Force` of the own notes folder. Claude
//! Code 2.1.293 allows such a command only as read-only inside a working folder, and in `auto`
//! it hands every such allow to Anthropic's server-side classifier (`Hln`, `nTr`, on while
//! `sO`), whose verdict comes back with the reply; a command in no rule goes to the auto-mode
//! classifier (`Vfe`, `Rdo`, its own Messages request). A command an allow rule matches skips
//! both: the auto-mode wrapper (`fTr`) returns an allow at once, and a rule's allow (`type:
//! "rule"`) is never rerouted. Only when every part of a command is allowed by a rule, though:
//! one read-only part without a rule (`| head -5`) puts the whole command back.
//!
//! What a rule allows is one command that lists or reads, followed by anything, then an own
//! folder's path in one of its literal spellings, then the end or a name boundary and anything:
//! `ls *C:\…\agent-state\<id>\notes`, `… notes\*`, `… notes"*`. The commands: `ls`, `cat` and
//! `head` in Bash, `Get-ChildItem` and `Get-Content` in PowerShell, which Claude Code matches by
//! their aliases too (`gci`, `dir`, `ls`, `gc`, `type`, `cat`: `yt` maps a command's and a
//! rule's first word through `z_`). Only commands Claude Code checks the paths of may be here
//! ([`BASH`], [`POWERSHELL`]): Bash's `dir` is not one, so `dir <any folder> <own notes>` would
//! list any folder with no question. A rule starts with the command's name, never with `*`,
//! so no other command can come first. Claude's `*` takes any text, so the other words of the
//! same command are anything; what keeps that narrow is Claude Code's own checks, which a rule
//! does not override:
//! - a deny rule beats an allow (`N_e`, `zin`: deny, then ask, then allow), so every
//!   protected place a command names in it is still denied ([`super`]'s rules);
//! - a compound command (`;`, `&&`, `||`, `|`, a newline) is allowed only when every part is
//!   (`g4e`; a wildcard rule never matches the compound text whole, `f9`), and a part no rule
//!   allows goes to a classifier;
//!   `$( )`, backticks, `<( )`, a variable, a function, `eval` or `source`, a subshell or an
//!   unquoted heredoc is too complex to match, and asks (`g4e`); PowerShell asks for a command
//!   it can't parse, a subexpression or parenthesis, a script block or a file redirection (`Ht`);
//! - a Bash redirection to a file asks whatever the rule (`a9`); and for each command here, so
//!   does every path argument outside the working folders (`additionalDirectories` holds the work
//!   and notes folders): Claude Code takes each of `ls`, `cat` and `head`'s arguments from
//!   the first that is not a flag on as a path, and each of `Get-ChildItem` and `Get-Content`'s
//!   positional and path arguments (an unknown parameter asks), so `cat <own>\x C:\secret`
//!   still goes to a classifier (checked off Windows with `/etc`, `scripts/claude-rule-probe`);
//! - an environment prefix other than Claude Code's safe ones (`PATH=…`) is not stripped before
//!   matching (`i_`, `S1`), so it matches no rule.
//!
//! The own path ends at a name boundary (a separator, a quote, a space, the end), so a sibling
//! whose name the own one starts (`run\…-1` and `…-10`, `notes` and `notes2`) is not the own
//! folder. Each path is written whole, with one separator throughout: `C:\…`, `C:/…`, Bash's
//! `C:\\…` and `/c/…`, for each spelling of `<data>` (its canonical path, and the path as
//! configured when that differs). A command that writes it any other way (a variable, `~`, a
//! mixed separator, a `.\` step) is in no rule and goes to the classifier, as before. The
//! PowerShell rules are written on Windows alone, where that tool exists; elsewhere the Bash
//! rules name the folders' paths, with `/`. On macOS the sandbox still wraps every command an
//! allow rule lets through: Claude Code's sandbox choice (`xy`) reads the input and the settings,
//! never the permission result, and `loads::sandbox` sets no `excludedCommands` and no
//! unsandboxed commands.
//!
//! A glob: Claude Code resolves links before it checks a literal path, but for a path with a
//! glob it checks only the folder before the glob, never what the glob expands to. The agent
//! can write into the own folders, so a link it makes there to a denied place, read through
//! `cat "<notes>"/*/x`, would match an allow rule and run unjudged. The same goes for a glob
//! anywhere else in an allowed command (`cat <matter>/*/x <notes>`), since the rule's `*`
//! takes it. So beside the allow rules, deny rules ([`own_shell_glob_deny_rules`], in
//! `permissions.deny` of a launch with no shell sandbox: Windows, or Claude's sandbox `Off`)
//! refuse each allowed command and path with a literal `*`, `?` or `[` before or after the
//! path: `cat *\**<notes>*`, `cat *<notes>*\**`. A deny beats the allow, and Claude retries
//! without the glob. A deny is matched on the whole command too, so on Windows an own read piped
//! through a glob (`cat <notes>/a.md | grep 'a.*b'`) is refused as well, until the own folders'
//! shell rules go there (after #1098); a sandboxed launch gets none, as Seatbelt checks the real
//! path a glob reaches. Each rule names its command
//! (matched through `z_` like the allow), so no other command that names an own folder with a
//! glob (`find <notes> -name '*.md'`, `cp <work>/*.docx <matter>`) is refused: those were never
//! allowed and still go to a classifier. A brace (`<notes>/{a,b}/x`) needs no deny: no allow
//! rule let one through (checked with `scripts/claude-rule-probe`, which sent it to a
//! classifier). PowerShell's wildcards are the same three characters. Not covered: a recursive
//! listing that follows links (`ls -RL <notes>`, Windows PowerShell 5.1's `Get-ChildItem
//! -Recurse` through a junction) can list the names in a linked target, though not its contents.

use super::{GLOBS, Piece, join, parse, render, text};
use crate::agent::data_fence::LaunchDeny;

/// The commands that list or read, in Bash: each in Claude Code's table of the commands whose
/// paths it checks. In 2.1.293 (read from `claude-agent-sdk-linux-x64` 0.3.293's
/// `package/claude`, sha256 `8968405e…`; names are minified per build, so find them by the
/// strings): `var Jye=Object.keys(D9),W_r={cd:"change directories to",ls:"list files in",…,
/// cat:"concatenate files from",head:"read the beginning of files from",…}` at byte 215488492,
/// 54 commands, no `dir`; a command not in it passes the check (`Command '${z}' is not a
/// path-restricted command`, byte 215502012). `D9`'s `ls`, `cat` and `head` take every
/// argument from the first that is not a flag on.
const BASH: [&str; 3] = ["ls", "cat", "head"];

/// The commands that list or read, in PowerShell (their aliases match the same rules): each in
/// Claude Code's table of the cmdlets whose paths it checks, `var ut=5,pt={"set-content":
/// {operationType:"write",…},…,"get-content":{operationType:"read",pathParams:["-path",…]},
/// "get-childitem":{operationType:"read",…}}` (byte 227625901 of the same file), read by `Ft`
/// (`function Ft(e){let n=z_(e.name),s=Aa(pt,n)`, byte 227644441): every positional argument
/// and path parameter is a path, and an unknown parameter makes the path unvalidatable (asks).
const POWERSHELL: [&str; 2] = ["Get-ChildItem", "Get-Content"];

/// What may follow an own folder's path, besides the end: a separator, a quote, a space.
const BOUNDARY: [&str; 5] = ["\\", "/", "\"", "'", " "];

/// One way of writing an own folder: `<data>`'s spelling, the separator it goes on with, and
/// whether PowerShell writes it too.
struct Lead {
    text: String,
    sep: &'static str,
    powershell: bool,
}

/// The own folders' paths (`deny`'s [`LaunchDeny::own_reads`]) under each of its spellings of
/// `<data>`, as rule pieces, each with whether PowerShell writes it too: Windows' spellings
/// (`C:\…`, `C:/…`, Bash's `C:\\…` and `/c/…`) when `windows`, else the path with `/`. A
/// spelling that is not UTF-8 gets none.
fn own_paths(deny: &LaunchDeny, windows: bool) -> Vec<(Vec<Piece>, bool)> {
    paths_of(deny, &deny.own_reads, windows)
}

/// [`own_paths`] for `places`, patterns under `<data>` in [`LaunchDeny::own_reads`]' form.
fn paths_of(deny: &LaunchDeny, places: &[String], windows: bool) -> Vec<(Vec<Piece>, bool)> {
    let mut leads: Vec<Lead> = Vec::new();
    let mut push = |text: String, sep: &'static str, powershell: bool| {
        if !text.is_empty() && !leads.iter().any(|known| known.text == text) {
            leads.push(Lead {
                text,
                sep,
                powershell,
            });
        }
    };
    for spelling in deny
        .spellings
        .iter()
        .filter_map(|spelling| spelling.to_str())
    {
        if !windows {
            push(spelling.trim_end_matches('/').to_owned(), "/", false);
            continue;
        }
        let back = crate::network_paths::resolve::drive_form(spelling)
            .unwrap_or_else(|| spelling.replace('/', "\\"));
        let back = back.trim_end_matches('\\');
        let forward = back.replace('\\', "/");
        push(back.to_owned(), "\\", true);
        push(forward.clone(), "/", true);
        push(back.replace('\\', "\\\\"), "\\\\", false);
        let mut chars = forward.chars();
        if let (Some(drive), Some(':')) = (chars.next(), chars.next())
            && drive.is_ascii_alphabetic()
        {
            push(
                format!("/{}{}", drive.to_ascii_lowercase(), chars.as_str()),
                "/",
                false,
            );
        }
    }
    let mut paths = Vec::new();
    for place in places {
        let (names, _) = parse(place);
        for lead in &leads {
            let mut path = text(&lead.text);
            for name in &names {
                path = join(&[&path, &text(lead.sep), name]);
            }
            paths.push((path, windows && lead.powershell));
        }
    }
    paths
}

/// The tools and commands a path's rules are for: Bash's, and PowerShell's when it writes it.
fn tools(powershell: bool) -> &'static [(&'static str, &'static [&'static str])] {
    if powershell {
        &[("Bash", &BASH), ("PowerShell", &POWERSHELL)]
    } else {
        &[("Bash", &BASH)]
    }
}

/// The allow rules (see the module doc) for `deny`'s own work and notes folders
/// ([`LaunchDeny::own_reads`]) under each of its spellings of `<data>`, the PowerShell ones and
/// Windows' spellings when `windows`. A spelling that is not UTF-8 gets none.
pub(crate) fn own_shell_allow_rules(deny: &LaunchDeny, windows: bool) -> Vec<String> {
    let mut rules = Vec::new();
    let mut add = |rule: String| {
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    };
    for (path, powershell) in own_paths(deny, windows) {
        for (tool, commands) in tools(powershell) {
            for command in *commands {
                let head = join(&[&text(&format!("{command} ")), &[Piece::Any], &path]);
                add(format!("{tool}({})", render(&head, true)));
                for after in BOUNDARY {
                    let pieces = join(&[&head, &text(after), &[Piece::Any]]);
                    add(format!("{tool}({})", render(&pieces, false)));
                }
            }
        }
    }
    rules
}

/// The deny rules (the module doc, "A glob") that refuse each allowed command naming an own
/// folder's path with a glob character ([`GLOBS`]: `*`, `?`, `[`) anywhere before or after
/// it: for each path and command of [`own_shell_allow_rules`], `<command> *<glob>*<path>*` and
/// `<command> *<path>*<glob>*`, the glob a literal (`cat *C:\…\notes*\**`). A spelling that is
/// not UTF-8 gets none.
pub(crate) fn own_shell_glob_deny_rules(deny: &LaunchDeny, windows: bool) -> Vec<String> {
    let mut rules = Vec::new();
    for (path, powershell) in own_paths(deny, windows) {
        for (tool, commands) in tools(powershell) {
            for command in *commands {
                let head = join(&[&text(&format!("{command} ")), &[Piece::Any]]);
                for glob in GLOBS {
                    let glob = text(glob);
                    for pieces in [
                        join(&[&head, &glob, &[Piece::Any], &path]),
                        join(&[&head, &path, &[Piece::Any], &glob]),
                    ] {
                        let rule = format!("{tool}({})", render(&pieces, false));
                        if !rules.contains(&rule) {
                            rules.push(rule);
                        }
                    }
                }
            }
        }
    }
    rules
}

/// The read-only `git` commands of the instructions (`legal.md`, "Version history").
const HISTORY_READS: [&str; 3] = ["log", "show", "diff"];

/// The options that would take an allowed history read past the history: another file read
/// (`diff --no-index`), a file written anywhere (`--output`), and a command run (`archive
/// --remote … --exec …`). git takes no abbreviation of any of them here: `--no-ind…` and
/// `--outp…` are ambiguous, and `archive` looks for `--remote` and `--exec` whole.
const HISTORY_DENIED: [&str; 4] = ["--no-index", "--output", "--exec", "--remote"];

/// The allow rules for reading this matter's version history (`agent::history`, found from the
/// notes in [`LaunchDeny::own_reads`]) with `git`, as the instructions write each command:
/// `git --git-dir="<history>" log *`, `show *` and `diff *`, so `auto` runs them with no
/// classifier (`scripts/claude-rule-probe`, Claude Code 0.3.293: the literal path matched, a
/// `$VAR` form did not). Bash's off Windows; PowerShell's on Windows, which matches a command
/// with its quotes dropped, so its rules come with and without them. No other matter's.
pub(crate) fn history_allow_rules(deny: &LaunchDeny, windows: bool) -> Vec<String> {
    prefix_rules(deny, windows, |head| {
        HISTORY_READS
            .iter()
            .map(|read| join(&[head, &text(&format!(" {read} ")), &[Piece::Any]]))
            .collect()
    })
}

/// The deny rules that keep [`history_allow_rules`] from letting more through than Claude
/// Code's classifier would: the same prefix with any of [`HISTORY_DENIED`] after it
/// (`git --git-dir="<history>" *--output*`). A deny beats an allow; no other command is touched.
pub(crate) fn history_deny_rules(deny: &LaunchDeny, windows: bool) -> Vec<String> {
    prefix_rules(deny, windows, |head| {
        HISTORY_DENIED
            .iter()
            .map(|option| {
                join(&[
                    head,
                    &text(" "),
                    &[Piece::Any],
                    &text(option),
                    &[Piece::Any],
                ])
            })
            .collect()
    })
}

/// [`history_allow_rules`] and [`history_deny_rules`] for one launch's settings, Bash's off
/// Windows and PowerShell's on Windows (`windows`): this matter's history alone, so they do not
/// grow with matters.
pub(crate) fn history_rules(deny: &LaunchDeny, windows: bool) -> (Vec<String>, Vec<String>) {
    (
        history_allow_rules(deny, windows),
        history_deny_rules(deny, windows),
    )
}

/// The rules `tails` makes from each spelling of the prefix `git --git-dir="<history>"`.
fn prefix_rules(
    deny: &LaunchDeny,
    windows: bool,
    tails: impl Fn(&[Piece]) -> Vec<Vec<Piece>>,
) -> Vec<String> {
    let places: Vec<String> = deny
        .own_reads
        .iter()
        .filter_map(|place| crate::agent::history::history_place(place))
        .collect();
    let mut rules = Vec::new();
    for (path, powershell) in paths_of(deny, &places, windows) {
        let (tool, quotes): (&str, &[&str]) = match (windows, powershell) {
            (false, _) => ("Bash", &["\""]),
            (true, true) => ("PowerShell", &["\"", ""]),
            (true, false) => continue,
        };
        for quote in quotes {
            let head = join(&[
                &text(&format!("git --git-dir={quote}")),
                &path,
                &text(quote),
            ]);
            for pieces in tails(&head) {
                let rule = format!("{tool}({})", render(&pieces, false));
                if !rules.contains(&rule) {
                    rules.push(rule);
                }
            }
        }
    }
    rules
}

#[cfg(test)]
#[path = "own_allow_tests.rs"]
mod tests;
