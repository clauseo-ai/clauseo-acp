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

use super::{Piece, join, parse, render, text};
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

/// The allow rules (see the module doc) for `deny`'s own work and notes folders
/// ([`LaunchDeny::own_reads`]) under each of its spellings of `<data>`, the PowerShell ones and
/// Windows' spellings when `windows`. A spelling that is not UTF-8 gets none.
pub(crate) fn own_shell_allow_rules(deny: &LaunchDeny, windows: bool) -> Vec<String> {
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
    let mut rules = Vec::new();
    let mut add = |rule: String| {
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    };
    for place in &deny.own_reads {
        let (names, _) = parse(place);
        for lead in &leads {
            let mut path = text(&lead.text);
            for name in &names {
                path = join(&[&path, &text(lead.sep), name]);
            }
            let tools: &[(&str, &[&str])] = if windows && lead.powershell {
                &[("Bash", &BASH), ("PowerShell", &POWERSHELL)]
            } else {
                &[("Bash", &BASH)]
            };
            for (tool, commands) in tools {
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
    }
    rules
}

#[cfg(test)]
#[path = "own_allow_tests.rs"]
mod tests;
