//! Windows: Claude's `Bash(…)` and `PowerShell(…)` deny rules for a command that names what of
//! the data folder is not the session's own (decision record 0030, "Claude's permission mode").
//! Claude runs in `auto` on Windows too, with no sandbox there, so a command in no rule goes to
//! the classifier alone; these rules refuse the plain spellings first, beside the `Read` and
//! `Edit` rules of [`super::claude_deny_rules`], and name the same targets (`LaunchDeny`,
//! `launch.rs`): [`super::DENIED`] and every entry of `<data>` the launch found that is not the
//! session's own. So the session's run folder, this matter's notes, Claude's plans folder and its
//! project's `tool-results` stay reachable by a command, and everything else under `<data>` is
//! denied when a command names it, the data folder itself too. That includes what every session
//! shares ([`super::SHARED`], [`super::READ_ONLY`]: the built-in plugin, the agents' commands,
//! Clauseo's Python runtime), which Claude reads with its Read tool and runs by name on its
//! `PATH` (`pandoc`, `stetdoc`, `python`, `agent-browser`); a command that names them could
//! write there, into every matter. A rule has no exceptions, so a rule for the whole folder would
//! deny the own folders with it.
//!
//! Claude Code 2.1.293 reads a rule in two steps (checked against the pinned binary through a
//! stand-in for the Messages API, `scripts/claude-rule-probe`; ported in `shell_deny_tests.rs`).
//! Its parser (`jn`) passes the content through `Lho`: `\(` and `\)` become parentheses and
//! every `\\` becomes `\`. Its matcher (`hQ`) then reads the result as an anchored regular
//! expression: `*` is any text, spaces and separators included, a leading one too; `\*` is a
//! literal `*` and `\\` a literal `\`; any other `\` stands for itself; runs of spaces and tabs
//! count as one; and `/**/` is a globstar (no rule here holds one). So a backslash in a rule is
//! written once before a letter and as 2k − 1 for k the matcher must see ([`render`]). A rule
//! that does not end in `*` must reach the end of the command. Bash rules match
//! case-sensitively, PowerShell rules in any case (`yt` passes `hQ` its case-insensitive flag) and
//! against each command rebuilt from its name and arguments. A scoped PowerShell rule also keeps
//! the PowerShell tool on beside the Bash deny rules, which would otherwise turn it off
//! (code.claude.com/docs/en/tools-reference, "Bash deny rules also turn off the PowerShell tool").
//!
//! The rules, for each way of writing `<data>` ([`forms`]), each separator after it
//! ([`JUNCTIONS`]: `\`, `/`, `\\`), both tools, each once:
//! - each target: the form, a separator, the target's path: its names with Claude's `*` between
//!   them, so any run or mix of separators and `.\` steps matches, the last name after a
//!   separator ([`spread`]). A sibling's name may be a text prefix of an own one (run `…-1`
//!   beside the own `…-10`; a matter folder `X` and another `X 2`, whose Claude projects are
//!   `…-X` and `…-X-2`). Where one of the launch's kept places could start with the target's
//!   name ([`LaunchDeny::kept`]), the rule ends at a name boundary: the name followed by a
//!   separator or one of [`AFTER`] (a quote, a space or tab, a newline, `;`, `|`, `&`, `>`,
//!   `<`, `(`, `)`, `,`, a backtick, `*`, `?`, `[`, a dot, a colon), then anything, and the
//!   name at the end of the command. Claude's `*` takes any text, so no fewer rules can say "one
//!   of these, or the end"; a PowerShell command loses its quotes before it is matched, and
//!   Claude Code matches each part of a compound Bash command on its own (`XK`), so the space
//!   and end rules carry most of it. Elsewhere one rule, the name followed by anything.
//! - the data folder named alone, and each folder the launch walked into (`run`, `agent-state`,
//!   this matter's state, `agents` and Claude's folders above its plans and projects): the name
//!   followed by one of [`AFTER`] or at the end (a recursive command there reaches every
//!   sibling; Win32 drops a trailing dot or space, so `ai.clauseo.desktop.\tools` is the data
//!   folder's), and by a separator at the end, or before a quote, a space, another separator, a
//!   dot or a glob character ([`after_sep`]: `<data>\.\tools`, `<data>//profile`). The walked
//!   folders' own rules are why a name right after one of them may start after a single
//!   separator: a run or a `.\` step there is denied by them.
//! - after the data folder, anything, then `\..` or `/..`: a `..` anywhere after it, with any
//!   separators, which could climb out of an own folder into a sibling, whatever it costs a
//!   command that climbs within an own one (`pandoc -o ..\..\x` there is denied too).
//!
//! A target is a pattern of `launch.rs`: its final `/**` is dropped, the gitignore escapes are
//! undone, and a pattern `*` (`run/*/relay-token`, `state.sqlite*`, a group of siblings
//! `run/<start>*`) stays Claude's `*`, which takes separators too (`run\<own>\work\relay-token.md`
//! is denied with the token; the accepted cost). A target's name is matched as a start, so a
//! trailing dot or space and a stream (`settings.json::$DATA`) are denied with it. What lies
//! under Clauseo's own names ([`OWN_WORDS`]: `agent-state`, `claude-acp`, `codex-acp`), and a
//! run folder whose name (or group's start) is an id's, is named by its tail alone
//! (`agent-state\<id>`, `claude-acp\home\projects\<key>`, `run\<id>`), whatever spelling of
//! `<data>` is before it: no folder of the lawyer's holds those names, and with `launch.rs`
//! naming the siblings of a busy folder in groups, a launch's rules hardly grow with the matters
//! (the settings file Claude Code reads is at most 2 MiB, `agent::launch_settings`). The
//! default data folder is written as Clauseo's own folder name alone
//! (`clauseo_settings::IDENTIFIER`), which every other way of writing it (a drive, a share or
//! device path, `%LOCALAPPDATA%` in any case, `~`, `$HOME`, …) ends in.
//!
//! A deny list is not a fence. Not caught, the accepted residual of `auto` on Windows (decision
//! record 0030): a path built at run time, a variable of the agent's own, a `cd` first (a `cd`
//! into the own run folder, then `cd ..` twice), a glob inside a name (`run\01a1*`), other
//! letter cases in Bash (`AI.CLAUSEO.DESKTOP`), an 8.3 short name, a script that opens the file,
//! a run folder, matter or project made after the launch whose name starts no group, and, for a
//! configured data folder (not `ai.clauseo.desktop`) alone, its own path written other than
//! whole: a `..` or `.\` step or a mixed separator inside it (`D:\FirmData\.\Clauseo`), a share or
//! device path to it (`\\localhost\D$\…`) or a variable other than `forms::VARS` naming part of
//! it. Denied beyond the data folder (accepted): any command naming `claude-acp` or
//! `agent-state` alone (`npm ls claude-acp`), and in PowerShell, which ignores case, a path such
//! as `…\Projects\Deal\` after `claude-acp` and `home` in the same command.

use std::collections::HashSet;
use std::ffi::OsString;

use super::LaunchDeny;
use forms::{Shells, forms};

#[path = "shell_deny_forms.rs"]
mod forms;

/// The separators a command may write between two names: `\`, `/`, and `\\` (Git Bash's escaped
/// form, or a doubled `\`).
const JUNCTIONS: [&str; 3] = ["\\", "/", "\\\\"];

/// What may follow a name at a boundary, besides a separator or the end: a quote, a space or
/// tab, a newline, what ends a command or starts a redirection, a list, a group or a command
/// substitution, a glob character (`run\<sibling>*`), a dot (Win32 drops a name's trailing dots
/// and spaces: `ai.clauseo.desktop.\tools` is the data folder's) and a colon (a stream,
/// `settings.json::$DATA`).
const AFTER: [&str; 19] = [
    "\"", "'", " ", "\t", "\n", ";", "|", "&", ">", "<", "(", ")", ",", "`", "*", "?", "[", ".",
    ":",
];

/// What may follow a separator after a folder named alone: a quote or a space (`dir "<data>\"`),
/// a `$`, `{` or backtick, which a shell may splice a name with (`<data>\$'tools'`,
/// `<data>/{tools,x}`), another separator or a dot (`<data>\\.\tools`, `<data>//profile`,
/// `<data>\/agents`). Not `\`
/// after `\`: `\\` is the escaped form's separator, which the rules name in its own right.
/// A `strict` folder is one no own path is written past in the escaped form with a single `\`
/// after it (a configured folder's path, `D:\FirmData\Clauseo\\x`): there `\` after `\` is a run.
fn after_sep(end: &str, strict: bool) -> &'static [&'static str] {
    match end {
        "\\" if strict => &["\"", "'", " ", "/", ".", "$", "{", "`", "\\"],
        "\\" => &["\"", "'", " ", "/", ".", "$", "{", "`"],
        "/" => &["\"", "'", " ", "/", "\\", ".", "$", "{", "`"],
        _ => &["\"", "'", " ", "\\", "/", ".", "$", "{", "`"],
    }
}

/// The separators a `..` may follow anywhere after the data folder's name (`\\..` holds `\..`).
const DOTS: [&str; 2] = ["\\", "/"];

/// The glob characters that reach every entry of a folder.
const GLOBS: [&str; 3] = ["*", "?", "["];

/// A piece of a rule: text that stands for itself, or Claude's `*`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    Any,
}

/// The rules (see the module doc) for `deny`'s spellings of `<data>` and targets, with `var`
/// reading the variables of `forms::VARS`.
pub(crate) fn windows_shell_deny_rules(
    deny: &LaunchDeny,
    var: &dyn Fn(&str) -> Option<OsString>,
) -> Vec<String> {
    rules(deny, var, true)
}

/// [`windows_shell_deny_rules`] without the forms that write a configured data folder through a
/// variable (`%APPDATA%`, `$env:USERPROFILE`, `~`, …): what a launch sheds when its settings would
/// outgrow the file Claude Code reads (`agent::loads::profile`). The default folder loses
/// nothing: its name alone stands for all of those.
pub(crate) fn windows_shell_deny_rules_without_variables(
    deny: &LaunchDeny,
    var: &dyn Fn(&str) -> Option<OsString>,
) -> Vec<String> {
    rules(deny, var, false)
}

/// [`windows_shell_deny_rules`], with the variable forms when `variables`.
fn rules(
    deny: &LaunchDeny,
    var: &dyn Fn(&str) -> Option<OsString>,
    variables: bool,
) -> Vec<String> {
    let forms = forms(deny, var, variables);
    // The shared and read-only places are the shell's to run by name on its `PATH`, never to
    // name: a command that writes there would reach every matter.
    let shared: Vec<String> = super::SHARED
        .iter()
        .map(|(place, _)| super::escape(place))
        .chain(super::READ_ONLY.iter().map(|place| (*place).to_owned()))
        .collect();
    let kept: Vec<Vec<Vec<Piece>>> = deny
        .kept
        .iter()
        .filter(|place| !shared.contains(place))
        .map(|place| parse(place).0)
        .collect();
    let mut targets: Vec<String> = deny.targets.clone();
    for place in &shared {
        let target = format!("{place}/**");
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    let mut parents: Vec<Vec<Vec<Piece>>> = Vec::new();
    for place in &kept {
        for end in 1..place.len() {
            let Some(parent) = place.get(..end) else {
                continue;
            };
            let literal = parent.iter().all(|name| !name.contains(&Piece::Any));
            if literal && !parents.iter().any(|known| known.as_slice() == parent) {
                parents.push(parent.to_vec());
            }
        }
    }
    // Whether the folder above `names`' last is one the launch walked into: its own rules
    // then catch a doubled separator or a `.\` step after it, and the last name may start
    // right after a separator ([`spread`]).
    let walked = |names: &[Vec<Piece>]| {
        names.split_last().is_some_and(|(_, above)| {
            !above.is_empty() && parents.iter().any(|parent| parent.as_slice() == above)
        })
    };
    // Each rule's pieces, whether it is for the end of the command, and its shells.
    let mut tagged: Vec<(Vec<Piece>, bool, Shells)> = Vec::new();
    let mut take = |contents: &mut Vec<Vec<Piece>>, ends: &mut Vec<Vec<Piece>>, shells: Shells| {
        tagged.extend(contents.drain(..).map(|pieces| (pieces, false, shells)));
        tagged.extend(ends.drain(..).map(|pieces| (pieces, true, shells)));
    };
    let mut contents: Vec<Vec<Piece>> = Vec::new();
    let mut ends: Vec<Vec<Piece>> = Vec::new();
    for form in &forms {
        let lead = text(&form.lead);
        for dots in DOTS {
            contents.push(join(&[&lead, &[Piece::Any], &text(dots), &text("..")]));
        }
        // Clauseo's own folder name ends the escaped form too (`…\\ai.clauseo.desktop\\run`),
        // and the escaped form is written with `\\` throughout.
        let strict = form.lead != clauseo_settings::IDENTIFIER && !form.lead.contains("\\\\");
        alone(&lead, strict, &mut contents, &mut ends);
        for junction in form.junctions {
            let lead_sep = text(&format!("{}{junction}", form.lead));
            for parent in parents.iter().filter(|parent| tail(parent).is_none()) {
                for path in spread(parent, walked(parent)) {
                    alone(&join(&[&lead_sep, &path]), strict, &mut contents, &mut ends);
                }
            }
        }
        take(&mut contents, &mut ends, form.shells);
    }
    for parent in &parents {
        if let Some(tail) = tail(parent) {
            for path in spread(tail, walked(parent)) {
                alone(&path, false, &mut contents, &mut ends);
            }
        }
    }
    take(&mut contents, &mut ends, Shells::Both);
    for target in &targets {
        let (names, _folder) = parse(target);
        let bounded = needs_boundary(&names, &kept);
        let exact = walked(&names);
        if let Some(tail) = tail(&names) {
            for path in spread(tail, exact) {
                named(&path, bounded, &mut contents, &mut ends);
            }
            take(&mut contents, &mut ends, Shells::Both);
            continue;
        }
        for form in &forms {
            for junction in form.junctions {
                let lead_sep = text(&format!("{}{junction}", form.lead));
                for path in spread(&names, exact) {
                    named(
                        &join(&[&lead_sep, &path]),
                        bounded,
                        &mut contents,
                        &mut ends,
                    );
                }
            }
            take(&mut contents, &mut ends, form.shells);
        }
    }
    // Each rule once per shell; PowerShell's in any case, as it matches them.
    let mut rules = Vec::new();
    for tool in ["Bash", "PowerShell"] {
        let mut seen = HashSet::new();
        for (pieces, end, shells) in &tagged {
            if !shells.has(tool) {
                continue;
            }
            let content = render(pieces, *end);
            let key = if tool == "PowerShell" {
                content.to_lowercase()
            } else {
                content.clone()
            };
            if seen.insert(key) {
                rules.push(format!("{tool}({content})"));
            }
        }
    }
    rules
}

/// The rules for a name (`path`) a target ends in: followed by anything, or when `bounded` at
/// a name boundary only (a separator, one of [`AFTER`], the end).
fn named(
    path: &[Piece],
    bounded: bool,
    contents: &mut Vec<Vec<Piece>>,
    ends: &mut Vec<Vec<Piece>>,
) {
    if !bounded {
        contents.push(join(&[path, &[Piece::Any]]));
        return;
    }
    for after in JUNCTIONS.into_iter().chain(AFTER) {
        contents.push(join(&[path, &text(after), &[Piece::Any]]));
    }
    ends.push(path.to_vec());
}

/// The rules for a folder (`path`) named alone: followed by one of [`AFTER`] or at the end, and
/// by a separator at the end, or before one of [`after_sep`] or a glob character.
fn alone(path: &[Piece], strict: bool, contents: &mut Vec<Vec<Piece>>, ends: &mut Vec<Vec<Piece>>) {
    for junction in JUNCTIONS {
        let with = join(&[path, &text(junction)]);
        for after in after_sep(junction, strict).iter().chain(&GLOBS) {
            contents.push(join(&[&with, &text(after), &[Piece::Any]]));
        }
        ends.push(with);
    }
    for after in AFTER {
        contents.push(join(&[path, &text(after), &[Piece::Any]]));
    }
    ends.push(path.to_vec());
}

/// `target`, a pattern of `launch.rs`, as names of pieces: escapes undone, a pattern `*` as
/// [`Piece::Any`]; and whether it named a folder (`/**`), which is dropped.
fn parse(target: &str) -> (Vec<Vec<Piece>>, bool) {
    let (path, folder) = match target.strip_suffix("/**") {
        Some(path) => (path, true),
        None => (target, false),
    };
    let names = path
        .split('/')
        .map(|name| {
            let mut pieces = Vec::new();
            let mut chars = name.chars();
            while let Some(c) = chars.next() {
                match c {
                    '*' => pieces.push(Piece::Any),
                    '\\' => push_text(&mut pieces, chars.next().unwrap_or('\\')),
                    _ => push_text(&mut pieces, c),
                }
            }
            pieces
        })
        .collect();
    (names, folder)
}

/// `c` added to the text at the end of `pieces`.
fn push_text(pieces: &mut Vec<Piece>, c: char) {
    if let Some(Piece::Text(text)) = pieces.last_mut() {
        text.push(c);
    } else {
        pieces.push(Piece::Text(c.to_string()));
    }
}

/// The names of Clauseo's own that no folder of the lawyer's holds: a target or folder under
/// one is named from it on ([`tail`]).
const OWN_WORDS: [&str; 3] = ["agent-state", "claude-acp", "codex-acp"];

/// How long a run folder's name, or its group's start (`launch.rs`), must be to name it by its
/// tail: an id's first twelve characters are no lawyer's folder name.
const DISTINCT: usize = 12;

/// The tail a target or a folder is named by alone (the module doc), whatever spelling of
/// `<data>` is before it: from the first of [`OWN_WORDS`] on (`agent-state\<id>`,
/// `claude-acp\home\projects\<key>`), or `run\<name>` when the name (or its group's start)
/// has at least [`DISTINCT`] characters.
fn tail(names: &[Vec<Piece>]) -> Option<&[Vec<Piece>]> {
    fn is(name: &[Piece], word: &str) -> bool {
        matches!(name, [Piece::Text(text)] if text == word)
    }
    if let Some(at) = names
        .iter()
        .position(|name| OWN_WORDS.iter().any(|word| is(name, word)))
    {
        return names.get(at..);
    }
    match names {
        [run, name] if is(run, "run") => {
            let ([Piece::Text(start)] | [Piece::Text(start), Piece::Any]) = name.as_slice() else {
                return None;
            };
            (start.chars().count() >= DISTINCT).then_some(names)
        }
        _ => None,
    }
}

/// Whether a kept place could have a name at `names`' last that starts with it and is longer:
/// then its rules end at a name boundary. A target whose last name holds a pattern `*` never
/// does. Names compare in any case, as Windows does; a pattern is taken as meeting anything.
fn needs_boundary(names: &[Vec<Piece>], kept: &[Vec<Vec<Piece>>]) -> bool {
    let Some((last, above)) = names.split_last() else {
        return false;
    };
    let Some(name) = lead_text(last).filter(|_| !last.contains(&Piece::Any)) else {
        return false;
    };
    kept.iter().any(|place| {
        let meets = |(target, place): (&Vec<Piece>, &Vec<Piece>)| {
            target.contains(&Piece::Any) || place.contains(&Piece::Any) || target == place
        };
        let Some(at) = place.get(above.len()) else {
            return false;
        };
        let start = lead_text(at).unwrap_or_default();
        let extends = if at.contains(&Piece::Any) {
            start.starts_with(&name) || name.starts_with(&start)
        } else {
            start.starts_with(&name) && start != name
        };
        above.iter().zip(place).all(meets) && extends
    })
}

/// The text a name starts with, folded to lower case: the whole name when it has no pattern.
fn lead_text(name: &[Piece]) -> Option<String> {
    match name.first() {
        Some(Piece::Text(text)) => Some(text.to_lowercase()),
        Some(Piece::Any) => Some(String::new()),
        None => None,
    }
}

/// `names` as a command may write them: one after another, each but the last followed by
/// Claude's `*` (any separators, `.\` steps among them). The last starts right after a
/// separator of [`JUNCTIONS`] when the folder above it is `walked` (its own rules catch a
/// doubled separator or a dot after it), so a group's start (`run\x*`) can't match inside an
/// own name; otherwise after `*` and `\` or `/`, so a separator run before it matches too.
fn spread(names: &[Vec<Piece>], walked: bool) -> Vec<Vec<Piece>> {
    let Some((last, above)) = names.split_last() else {
        return Vec::new();
    };
    if above.is_empty() {
        return vec![last.clone()];
    }
    let mut inner = Vec::new();
    for (n, name) in above.iter().enumerate() {
        if n > 0 {
            inner.push(Piece::Any);
        }
        inner.extend(name.iter().cloned());
    }
    if walked {
        return JUNCTIONS
            .into_iter()
            .map(|junction| join(&[&inner, &text(junction), last]))
            .collect();
    }
    ["\\", "/"]
        .into_iter()
        .map(|end| join(&[&inner, &[Piece::Any], &text(end), last]))
        .collect()
}

/// One text piece.
fn text(text: &str) -> Vec<Piece> {
    vec![Piece::Text(text.to_owned())]
}

/// `parts` one after another.
fn join(parts: &[&[Piece]]) -> Vec<Piece> {
    parts.iter().flat_map(|part| part.iter().cloned()).collect()
}

/// A rule's content: a leading `*`, the pieces, and a trailing `*` unless the rule is for the end
/// of the command (`end`), written for Claude Code 2.1.293's two readings of it. Its rule
/// parser (`jn`) first passes the content through `Lho`: `\(` and `\)` become `(` and `)`, then
/// every `\\` becomes `\`. Its matcher (`hQ`) then reads `\*` as a literal `*`, `\\` as a literal
/// `\`, any other `\` as itself, and `*` as any text. So the pattern for `hQ` writes a literal `\`
/// as `\\` only before a `*` or a `\` (`run\\*`: a backslash, then any text) and as `\` elsewhere,
/// a literal `*` as `\*`; and each run of k backslashes in it is written as 2k − 1 for `Lho`
/// (`\\\\\` becomes `\\\`), or 2k before `(` or `)`, which `Lho` would otherwise take as an
/// escaped parenthesis, and at the end: an odd run there would escape the rule's closing `)`,
/// and Claude Code's loader would skip the rule as malformed (`lYe`; a folder named with a
/// separator at its end, `…\ai.clauseo.desktop\`). Checked against the pinned binary
/// (`shell_deny_tests.rs`, `scripts/claude-rule-probe`).
fn render(pieces: &[Piece], end: bool) -> String {
    /// One character of the pattern: a literal, or Claude's `*`.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Tok {
        Lit(char),
        Any,
    }
    let mut toks = vec![Tok::Any];
    for piece in pieces {
        match piece {
            Piece::Text(text) => toks.extend(text.chars().map(Tok::Lit)),
            Piece::Any if toks.last() == Some(&Tok::Any) => {}
            Piece::Any => toks.push(Tok::Any),
        }
    }
    if !end && toks.last() != Some(&Tok::Any) {
        toks.push(Tok::Any);
    }
    // The pattern `hQ` reads.
    let mut pattern = String::new();
    for (n, tok) in toks.iter().enumerate() {
        match tok {
            Tok::Any => pattern.push('*'),
            Tok::Lit('*') => pattern.push_str("\\*"),
            Tok::Lit('\\') => {
                let next = toks.get(n + 1);
                let doubled = matches!(next, Some(Tok::Any | Tok::Lit('*' | '\\')));
                pattern.push_str(if doubled { "\\\\" } else { "\\" });
            }
            Tok::Lit(c) => pattern.push(*c),
        }
    }
    // The content that `Lho` turns into that pattern.
    let mut content = String::with_capacity(pattern.len() * 2);
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            content.push(c);
            continue;
        }
        let mut run = 1;
        while chars.peek() == Some(&'\\') {
            chars.next();
            run += 1;
        }
        // Before a parenthesis or at the end, before the rule's closing `)`: an even run.
        let paren = matches!(chars.peek(), Some('(' | ')') | None);
        let written = if paren { 2 * run } else { 2 * run - 1 };
        content.extend(std::iter::repeat_n('\\', written));
    }
    // A content ending in `:*` is Claude Code's legacy prefix rule (`cCe`, `^(.+):\*$`): the
    // command must start with the rest, which no command does. `:**` is the same match.
    if content.ends_with(":*") {
        content.push('*');
    }
    content
}

#[cfg(test)]
#[path = "shell_deny_cross_tests.rs"]
mod cross_tests;
#[cfg(test)]
#[path = "shell_deny_index_tests.rs"]
mod index_tests;
#[cfg(test)]
#[path = "shell_deny_launch_tests.rs"]
mod launch_tests;
#[cfg(test)]
#[path = "shell_deny_real_tests.rs"]
mod real_tests;
#[cfg(test)]
#[path = "shell_deny_shape_tests.rs"]
mod shape_tests;
#[cfg(test)]
#[path = "shell_deny_tests.rs"]
mod tests;
