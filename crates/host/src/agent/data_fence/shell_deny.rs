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
//! Claude Code 2.1.293 matches a rule (`hQ`) as an anchored regular expression: `*` is any text,
//! spaces and separators included, a leading one too; `\*` is a literal `*` and `\\` a literal
//! `\`; any other character stands for itself; runs of spaces and tabs count as one; and `/**/`
//! is a globstar (no rule here holds one). A rule that does not end in `*` must reach the end of
//! the command. Bash rules match case-sensitively, PowerShell rules in any case
//! (`preparePermissionMatcher` passes `hQ` its case-insensitive flag) and against each command
//! rebuilt from its name and arguments. A scoped PowerShell rule also keeps the PowerShell tool
//! on beside the Bash deny rules, which would otherwise turn it off (code.claude.com/docs/en/
//! tools-reference, "Bash deny rules also turn off the PowerShell tool").
//!
//! The rules, for each spelling of `<data>` ([`forms`]) and its separators, both tools, each once:
//! - each target: the form, a separator, the target's path: its names with Claude's `*` between
//!   them, so any mix of `\` and `/` matches, but the last after a separator (`\` or `/`, or
//!   `\\` in the escaped form), so it still starts a name ([`spread`]). A sibling's name
//!   may be a text prefix of an own one (run `…-1` beside the own `…-10`; a matter folder `X` and
//!   another `X 2`, whose Claude projects are `…-X` and `…-X-2`). Where one of the launch's kept
//!   places could start with the target's name ([`LaunchDeny::kept`]), the rule ends at a name
//!   boundary: the name followed by `\` (`\\` in the escaped form), `/` or one of [`AFTER`]
//!   (a quote, a space or tab, a newline, `;`, `|`, `&`, `>`, `<`, `(`, `)`, `,`, a backtick,
//!   `*`, `?`, `[`), then anything, and the name at the end of the command. Claude's `*` takes
//!   any text, so no fewer rules can say "one of these, or the end"; a PowerShell command loses
//!   its quotes before it is matched, and Claude Code matches each part of a compound Bash
//!   command on its own (`XK`), so the space and end rules carry most of it. Elsewhere one rule,
//!   the name followed by anything.
//! - the data folder named alone, and each folder the launch walked into (`run`, `agent-state`,
//!   this matter's state, `agents` and Claude's folders above its plans and projects): the name
//!   followed by one of [`AFTER`] or at the end (a recursive command there reaches every
//!   sibling), and by a separator at the end, before a quote or a space, or before a glob
//!   character (`*`, `?`, `[`).
//! - after the data folder, anything, then `\..` or `/..`: a `..` anywhere after it, with any
//!   separators, which could climb out of an own folder into a sibling, whatever it costs a
//!   command that climbs within an own one (`pandoc -o ..\..\x` there is denied too).
//!
//! A target is a pattern of `launch.rs`: its final `/**` is dropped, `/` becomes the form's
//! separator, the gitignore escapes are undone, and a pattern `*` (`run/*/relay-token`,
//! `state.sqlite*`, a group of siblings `run/<start>*`) stays Claude's `*`, which takes
//! separators too (`run\<own>\work\relay-token.md` is denied with the token; the accepted
//! cost). What lies under Clauseo's own names ([`OWN_WORDS`]: `agent-state`, `claude-acp`,
//! `codex-acp`), and a run folder whose name (or group's start) is an id's, is named by its tail
//! alone (`agent-state\<id>`, `claude-acp\home\projects\<key>`, `run\<id>`), with each separator
//! and whatever spelling of `<data>` is before it: no folder of the lawyer's holds those names,
//! and with `launch.rs` naming the siblings of a busy folder in groups, a launch's rules hardly
//! grow with the matters (the settings file Claude Code reads is at most 2 MiB,
//! `agent::launch_settings`). A spelling that ends in Clauseo's own folder name
//! (`clauseo_settings::IDENTIFIER`) is left out beside that name alone: its rules would deny
//! only commands the name's rules already deny.
//!
//! A deny list is not a fence: a path built at run time, a variable of the agent's own, a `cd`
//! first (a `cd` into the own run folder, then `cd ..` twice), a glob inside a name, other
//! letter cases in Bash, an 8.3 short name, a separator other than its own right after a
//! configured folder's path (Clauseo's own folder name covers the default one) or a script that
//! opens the file are not caught, nor a run folder,
//! matter or project made after the launch whose name starts no group. That is the accepted
//! residual of `auto` on Windows (decision record 0030).

use std::collections::HashSet;
use std::ffi::OsString;

use super::LaunchDeny;

/// The variables a command may spell `<data>`'s parent with: `%LOCALAPPDATA%` holds the default
/// data folder, `%APPDATA%` a configured one may sit under.
const VARS: [&str; 2] = ["LOCALAPPDATA", "APPDATA"];

/// A separator as a command writes it: `\`, `/`, or `\\` (Git Bash's escaped form).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sep {
    Back,
    Forward,
    Escaped,
}

impl Sep {
    const ALL: [Self; 3] = [Self::Back, Self::Forward, Self::Escaped];

    fn text(self) -> &'static str {
        match self {
            Self::Back => "\\",
            Self::Forward => "/",
            Self::Escaped => "\\\\",
        }
    }

    /// The separators that may end a name written with this one: itself and `/` (a command may
    /// go on with `/` after a path written with `\`).
    fn ends(self) -> [&'static str; 2] {
        [self.text(), if self == Self::Forward { "\\" } else { "/" }]
    }
}

/// What may follow a name at a boundary, besides a separator or the end: a quote, a space or
/// tab, a newline, what ends a command or starts a redirection, a list, a group or a command
/// substitution, and a glob character (`run\<sibling>*`).
const AFTER: [&str; 17] = [
    "\"", "'", " ", "\t", "\n", ";", "|", "&", ">", "<", "(", ")", ",", "`", "*", "?", "[",
];

/// What may follow a separator after a folder named alone (`dir "<data>\"`): a quote, a space.
const AFTER_SEP: [&str; 3] = ["\"", "'", " "];

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

/// One way a command writes `<data>`, and the separators that may follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Form {
    lead: String,
    seps: Vec<Sep>,
}

/// The rules (see the module doc) for `deny`'s spellings of `<data>` and targets, with `var`
/// reading the variables of [`VARS`].
pub(crate) fn windows_shell_deny_rules(
    deny: &LaunchDeny,
    var: &dyn Fn(&str) -> Option<OsString>,
) -> Vec<String> {
    let forms = forms(deny, var);
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
    let mut contents: Vec<Vec<Piece>> = Vec::new();
    let mut ends: Vec<Vec<Piece>> = Vec::new();
    for form in &forms {
        let lead = text(&form.lead);
        for dots in DOTS {
            contents.push(join(&[&lead, &[Piece::Any], &text(dots), &text("..")]));
        }
        for &sep in &form.seps {
            alone(&lead, sep, &mut contents, &mut ends);
            let lead_sep = text(&format!("{}{}", form.lead, sep.text()));
            for parent in parents.iter().filter(|parent| tail(parent).is_none()) {
                for path in spread(parent, sep) {
                    alone(&join(&[&lead_sep, &path]), sep, &mut contents, &mut ends);
                }
            }
        }
    }
    for tail in parents.iter().filter_map(|parent| tail(parent)) {
        for sep in Sep::ALL {
            for path in spread(tail, sep) {
                alone(&path, sep, &mut contents, &mut ends);
            }
        }
    }
    for target in &targets {
        let (names, _folder) = parse(target);
        let bounded = needs_boundary(&names, &kept);
        if let Some(tail) = tail(&names) {
            for sep in Sep::ALL {
                for path in spread(tail, sep) {
                    named(&path, sep, bounded, &mut contents, &mut ends);
                }
            }
            continue;
        }
        for form in &forms {
            for &sep in &form.seps {
                let lead_sep = text(&format!("{}{}", form.lead, sep.text()));
                for path in spread(&names, sep) {
                    named(
                        &join(&[&lead_sep, &path]),
                        sep,
                        bounded,
                        &mut contents,
                        &mut ends,
                    );
                }
            }
        }
    }
    let mut seen = HashSet::new();
    let mut rules = Vec::new();
    for tool in ["Bash", "PowerShell"] {
        let all = contents
            .iter()
            .map(|pieces| render(pieces, false))
            .chain(ends.iter().map(|pieces| render(pieces, true)));
        for content in all {
            let rule = format!("{tool}({content})");
            if seen.insert(rule.clone()) {
                rules.push(rule);
            }
        }
    }
    rules
}

/// The rules for a name (`path`) a target ends in: followed by anything, or when `bounded` at
/// a name boundary only (a separator, a quote, a space, the end).
fn named(
    path: &[Piece],
    sep: Sep,
    bounded: bool,
    contents: &mut Vec<Vec<Piece>>,
    ends: &mut Vec<Vec<Piece>>,
) {
    if !bounded {
        contents.push(join(&[path, &[Piece::Any]]));
        return;
    }
    for after in sep.ends().into_iter().chain(AFTER) {
        contents.push(join(&[path, &text(after), &[Piece::Any]]));
    }
    ends.push(path.to_vec());
}

/// The rules for a folder (`path`) named alone: at a name boundary but a separator, and with
/// a separator at the end or before a quote or a space, or a separator and a glob character.
fn alone(path: &[Piece], sep: Sep, contents: &mut Vec<Vec<Piece>>, ends: &mut Vec<Vec<Piece>>) {
    for end in sep.ends() {
        let with = join(&[path, &text(end)]);
        for after in AFTER_SEP {
            contents.push(join(&[&with, &text(after), &[Piece::Any]]));
        }
        ends.push(with.clone());
        if end == sep.text() {
            for glob in GLOBS {
                contents.push(join(&[&with, &text(glob), &[Piece::Any]]));
            }
        }
    }
    for after in AFTER {
        contents.push(join(&[path, &text(after), &[Piece::Any]]));
    }
    ends.push(path.to_vec());
}

/// The ways a command writes each of `deny`'s spellings of `<data>`: its path with `\`, with
/// `/`, with `\\` and as Git Bash's `/c/…`; under a folder one of [`VARS`] names (read with
/// `var`, in any case), `%VAR%`, `$env:VAR`, `$VAR` and `${VAR}` followed by the rest; and, when
/// the folder has Clauseo's own name (`clauseo_settings::IDENTIFIER`), that name alone, which no
/// other folder has, followed by any separator. A form that ends in that name is then left out
/// (the module doc). A spelling that is not UTF-8 gets none.
fn forms(deny: &LaunchDeny, var: &dyn Fn(&str) -> Option<OsString>) -> Vec<Form> {
    let mut forms: Vec<Form> = Vec::new();
    let mut push = |lead: String, seps: Vec<Sep>| {
        if !lead.is_empty() && !forms.iter().any(|form| form.lead == lead) {
            forms.push(Form { lead, seps });
        }
    };
    let mut named = false;
    for spelling in &deny.spellings {
        let Some(text) = spelling.to_str() else {
            continue;
        };
        let back = drive_form(text);
        let back = back.trim_end_matches('\\');
        let forward = back.replace('\\', "/");
        push(back.to_owned(), vec![Sep::Back]);
        push(forward.clone(), vec![Sep::Forward]);
        push(back.replace('\\', "\\\\"), vec![Sep::Escaped]);
        let mut chars = forward.chars();
        if let (Some(drive), Some(':')) = (chars.next(), chars.next())
            && drive.is_ascii_alphabetic()
        {
            let git = format!("/{}{}", drive.to_ascii_lowercase(), chars.as_str());
            push(git, vec![Sep::Forward]);
        }
        for name in VARS {
            let Some(value) = var(name).and_then(|v| v.into_string().ok()) else {
                continue;
            };
            let value = drive_form(&value);
            let value = value.trim_end_matches('\\');
            let Some(rest) = strip_prefix_folded(back, value).and_then(|r| r.strip_prefix('\\'))
            else {
                continue;
            };
            if value.is_empty() || rest.is_empty() {
                continue;
            }
            let rest_forward = rest.replace('\\', "/");
            for lead in [format!("%{name}%"), format!("$env:{name}")] {
                push(format!("{lead}\\{rest}"), vec![Sep::Back]);
                push(format!("{lead}/{rest_forward}"), vec![Sep::Forward]);
            }
            for lead in [format!("${name}"), format!("${{{name}}}")] {
                push(format!("{lead}/{rest_forward}"), vec![Sep::Forward]);
            }
        }
        // The last name, split by hand: a Windows path is one name to `Path` off Windows.
        named |= back.rsplit('\\').next() == Some(clauseo_settings::IDENTIFIER);
    }
    if named {
        let id = clauseo_settings::IDENTIFIER;
        forms.retain(|form| {
            let before = form
                .lead
                .strip_suffix(id)
                .and_then(|rest| rest.chars().last());
            !matches!(before, Some('\\' | '/'))
        });
        forms.insert(
            0,
            Form {
                lead: id.to_owned(),
                seps: Sep::ALL.to_vec(),
            },
        );
    }
    forms
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

/// `names` as a command may write them with `sep`: one after another, each but the last
/// followed by Claude's `*` (any separator, `\` or `/` mixed in any way), the last after
/// `sep` or `/` (`\` after `/`), so that it still starts a name: a group's start (`run\x*`)
/// must not match inside an own name. Two variants for two or more names, one for one.
fn spread(names: &[Vec<Piece>], sep: Sep) -> Vec<Vec<Piece>> {
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
    sep.ends()
        .into_iter()
        .map(|end| join(&[&inner, &text(end), last]))
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

/// A rule's content: a leading `*`, the pieces (text escaped, runs of `*` as one), and a
/// trailing `*` unless the rule is for the end of the command (`end`).
fn render(pieces: &[Piece], end: bool) -> String {
    let mut out = String::from("*");
    for piece in pieces {
        match piece {
            Piece::Text(text) => out.push_str(&escape(text)),
            Piece::Any if ends_in_any(&out) => {}
            Piece::Any => out.push('*'),
        }
    }
    if !end && !ends_in_any(&out) {
        out.push('*');
    }
    out
}

/// Whether a rule's content so far ends in Claude's `*`, not a literal `\*`.
fn ends_in_any(content: &str) -> bool {
    content.ends_with('*') && !content.ends_with("\\*")
}

/// `text` in its drive form (`\\?\C:\x` is `C:\x`), `/` read as `\`.
fn drive_form(text: &str) -> String {
    crate::network_paths::resolve::drive_form(text).unwrap_or_else(|| text.replace('/', "\\"))
}

/// `text` without `prefix`, compared in any ASCII case, as Windows compares names.
fn strip_prefix_folded<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    let rest = text.get(prefix.len()..)?;
    head.eq_ignore_ascii_case(prefix).then_some(rest)
}

/// `text` in a Bash or PowerShell rule: `\` and `*` escaped by `\`.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
#[path = "shell_deny_launch_tests.rs"]
mod launch_tests;
#[cfg(test)]
#[path = "shell_deny_tests.rs"]
mod tests;
