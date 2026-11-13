//! Windows: Claude's `Bash(…)` and `PowerShell(…)` deny rules for a command that names what of
//! the data folder is not the session's own (decision record 0030, "Claude's permission mode").
//! Claude runs in `auto` on Windows too, with no sandbox there, so a command in no rule goes to
//! the classifier alone; these rules refuse the plain spellings first, beside the `Read` and
//! `Edit` rules of [`super::claude_deny_rules`], and name the same targets (`LaunchDeny`,
//! `launch.rs`): [`super::DENIED`] and every entry of `<data>` the launch found that is not the
//! session's own. So the session's run folder, this matter's notes, Claude's plans folder, its
//! project's `tool-results` and the read-only tools stay reachable by a command, and everything
//! else under `<data>` is denied when a command names it, the data folder itself too. A rule has
//! no exceptions, so a rule for the whole folder would deny the own folders with it.
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
//! - each target: the form, a separator, the target's path with that separator. A sibling's name
//!   may be a text prefix of an own one (run `…-1` beside the own `…-10`; a matter folder `X` and
//!   another `X 2`, whose Claude projects are `…-X` and `…-X-2`). Where one of the launch's kept
//!   places could start with the target's name ([`LaunchDeny::kept`]), the rule ends at a name
//!   boundary, as six rules: the name followed by `\` (`\\` in the escaped form), `/`, `"`, `'`
//!   or a space, then anything, and the name at the end of the command. Claude's `*` takes any
//!   text, so no fewer rules can say "one of these, or the end"; a PowerShell command loses its
//!   quotes before it is matched, so the space and end rules are the ones that hold there.
//!   Elsewhere one rule, the name followed by anything, which the six would only split.
//! - the data folder named alone, and each folder the launch walked into (`run`, `agent-state`,
//!   this matter's state, `agents` and Claude's folders above its plans and projects): the name
//!   with the boundaries above but the separators (a recursive command there reaches every
//!   sibling), and followed by a separator and a glob character (`*`, `?`, `[`).
//! - after the data folder, a separator, anything, then a separator and `..`.
//!
//! A target is a pattern of `launch.rs`: its final `/**` is dropped, `/` becomes the form's
//! separator, the gitignore escapes are undone, and a pattern `*` (`run/*/relay-token`,
//! `state.sqlite*`) stays Claude's `*`, which takes separators too (`run\<own>\work\relay-token.md`
//! is denied with the token; the accepted cost). A run folder or matter state named by its id
//! (`run/<uuid>-<n>`, `agent-state/<uuid>`) and a Claude project (`…/home/projects/<key>`) is
//! named by its tail alone (`run\<id>`, `home\projects\<key>`), whatever spelling of `<data>` is
//! before it: no folder of the lawyer's holds those names, and they are most of a busy data
//! folder's targets. A spelling that ends in Clauseo's own folder name
//! (`clauseo_settings::IDENTIFIER`) is left out beside that name alone: its rules would deny
//! only commands the name's rules already deny.
//!
//! A deny list is not a fence: a path built at run time, a variable of the agent's own, a `cd`
//! first, a glob inside a name, a `..` after mixed separators, other letter cases in Bash or a
//! script that opens the file are not caught. That is the accepted residual of `auto` on
//! Windows (decision record 0030).

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

/// What may follow a name at a boundary, besides a separator or the end.
const AFTER: [&str; 3] = ["\"", "'", " "];

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
    let kept: Vec<Vec<Vec<Piece>>> = deny.kept.iter().map(|place| parse(place).0).collect();
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
        for &sep in &form.seps {
            let lead = vec![Piece::Text(form.lead.clone())];
            alone(&lead, sep, &mut contents, &mut ends);
            let mut lead_sep = lead.clone();
            lead_sep.push(Piece::Text(sep.text().to_owned()));
            contents.push(join(&[
                &lead_sep,
                &[Piece::Any],
                &text(sep.text()),
                &text(".."),
            ]));
            for parent in &parents {
                let path = join(&[&lead_sep, &path(parent, sep)]);
                alone(&path, sep, &mut contents, &mut ends);
            }
        }
    }
    for target in &deny.targets {
        let (names, _folder) = parse(target);
        let bounded = needs_boundary(&names, &kept);
        match tail(&names) {
            Some(tail) => {
                for sep in Sep::ALL {
                    named(&path(tail, sep), sep, bounded, &mut contents, &mut ends);
                }
            }
            None => {
                for form in &forms {
                    for &sep in &form.seps {
                        let lead = text(&format!("{}{}", form.lead, sep.text()));
                        let full = join(&[&lead, &path(&names, sep)]);
                        named(&full, sep, bounded, &mut contents, &mut ends);
                    }
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
/// a separator at one, or a separator and a glob character.
fn alone(path: &[Piece], sep: Sep, contents: &mut Vec<Vec<Piece>>, ends: &mut Vec<Vec<Piece>>) {
    for end in sep.ends() {
        let with = join(&[path, &text(end)]);
        for after in AFTER {
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

/// The tail a target is named by alone (the module doc): `run/<id>`, `agent-state/<id>` or
/// `home/projects/<key>`, when it is one.
fn tail(names: &[Vec<Piece>]) -> Option<&[Vec<Piece>]> {
    fn literal(name: &[Piece]) -> Option<&str> {
        match name {
            [Piece::Text(text)] => Some(text.as_str()),
            _ => None,
        }
    }
    let texts: Vec<&str> = names
        .iter()
        .map(Vec::as_slice)
        .map(literal)
        .collect::<Option<_>>()?;
    match texts.as_slice() {
        ["run" | "agent-state", id] if is_id(id) => Some(names),
        ["agents", "claude-acp", "home", "projects", key] if !key.is_empty() => names.get(2..),
        _ => None,
    }
}

/// Whether `name` is a v7-style id (`8-4-4-4-12` hex digits), or one with `-<n>` after it as a
/// run folder's name.
fn is_id(name: &str) -> bool {
    let (id, rest) = name.split_at_checked(36).unwrap_or((name, ""));
    let groups: Vec<&str> = id.split('-').collect();
    let shaped = groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|group| group.bytes().all(|b| b.is_ascii_hexdigit()));
    let run = rest.is_empty()
        || rest
            .strip_prefix('-')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    shaped && run
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

/// `names` joined with `sep`.
fn path(names: &[Vec<Piece>], sep: Sep) -> Vec<Piece> {
    let mut out = Vec::new();
    for (n, name) in names.iter().enumerate() {
        if n > 0 {
            out.push(Piece::Text(sep.text().to_owned()));
        }
        out.extend(name.iter().cloned());
    }
    out
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
