//! The ways a command writes the data folder (`super`'s module doc): Clauseo's own folder name
//! alone, or a configured folder's path in each spelling and under each of the variables a
//! command may write its parent with, each with the separators that may follow it and the
//! shells that can write it.

use std::ffi::OsString;

use super::JUNCTIONS;
use crate::agent::data_fence::LaunchDeny;

/// The variables a command may spell `<data>`'s parent with: `%LOCALAPPDATA%` holds the default
/// data folder, `%APPDATA%` a configured one may sit under, and the lawyer's home
/// (`%USERPROFILE%`, `$HOME`, `~`) holds both.
const VARS: [&str; 4] = ["LOCALAPPDATA", "APPDATA", "USERPROFILE", "HOME"];

/// The separators after a form of `<data>` written with `/` (`C:/…`, `$HOME/…`): its own and
/// `\`, which a command may go on with.
const FORWARD: [&str; 2] = ["/", "\\"];

/// The separators after a form written with `\` and no doubled one (`%APPDATA%\…`).
const BACKWARD: [&str; 2] = ["\\", "/"];

/// One way a command writes `<data>` ([`forms`]), and the separators that may follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Form {
    pub(super) lead: String,
    pub(super) junctions: &'static [&'static str],
    pub(super) shells: Shells,
}

/// The shells a rule is for: a form of `<data>` that only one of them can write (`$env:VAR`
/// PowerShell, `$VAR` and `/c/…` Bash) is in that one's rules alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shells {
    Both,
    Bash,
    PowerShell,
}

impl Shells {
    pub(super) fn has(self, tool: &str) -> bool {
        match self {
            Self::Both => true,
            Self::Bash => tool == "Bash",
            Self::PowerShell => tool == "PowerShell",
        }
    }
}

/// The ways a command writes each of `deny`'s spellings of `<data>`: when the folder has
/// Clauseo's own name (`clauseo_settings::IDENTIFIER`), that name alone, which no other folder
/// has and which every other way of writing it ends in; otherwise its path with `\`, with `/`,
/// with `\\` and as Git Bash's `/c/…`, and under a folder one of [`VARS`] names (read with `var`,
/// in any case), `%VAR%` (and `%var%`), `$env:VAR`, `$VAR` and `${VAR}`, and `~` for the home,
/// followed by the rest. A spelling that is not UTF-8 gets none.
pub(super) fn forms(
    deny: &LaunchDeny,
    var: &dyn Fn(&str) -> Option<OsString>,
    variables: bool,
) -> Vec<Form> {
    let mut forms: Vec<Form> = Vec::new();
    let mut push = |lead: String, junctions: &'static [&'static str], shells: Shells| {
        if !lead.is_empty() && !forms.iter().any(|form| form.lead == lead) {
            forms.push(Form {
                lead,
                junctions,
                shells,
            });
        }
    };
    for spelling in &deny.spellings {
        let Some(text) = spelling.to_str() else {
            continue;
        };
        let back = drive_form(text);
        let back = back.trim_end_matches('\\');
        // The last name, split by hand: a Windows path is one name to `Path` off Windows.
        if back.rsplit('\\').next() == Some(clauseo_settings::IDENTIFIER) {
            push(
                clauseo_settings::IDENTIFIER.to_owned(),
                &JUNCTIONS,
                Shells::Both,
            );
            continue;
        }
        let forward = back.replace('\\', "/");
        push(back.to_owned(), &JUNCTIONS, Shells::Both);
        push(forward.clone(), &FORWARD, Shells::Both);
        // A doubled `\` is Bash's escape, and Win32 reads it as one `\` in any shell.
        push(back.replace('\\', "\\\\"), &JUNCTIONS, Shells::Both);
        let mut chars = forward.chars();
        if let (Some(drive), Some(':')) = (chars.next(), chars.next())
            && drive.is_ascii_alphabetic()
        {
            let git = format!("/{}{}", drive.to_ascii_lowercase(), chars.as_str());
            push(git, &FORWARD, Shells::Bash);
        }
        for name in VARS.iter().filter(|_| variables) {
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
            let lower = name.to_ascii_lowercase();
            // cmd's `%VAR%` (from either shell, through `cmd /c`), PowerShell's `$env:VAR`, Bash's
            // `$VAR` and `${VAR}`; the home's `$HOME` and `~` in both.
            let mut leads: Vec<(String, &'static [&'static str], Shells)> = Vec::new();
            if *name != "HOME" {
                leads.push((format!("%{name}%\\{rest}"), &BACKWARD, Shells::Both));
                leads.push((format!("%{lower}%\\{rest}"), &BACKWARD, Shells::Both));
                leads.push((
                    format!("$env:{name}\\{rest}"),
                    &BACKWARD,
                    Shells::PowerShell,
                ));
                leads.push((
                    format!("$env:{name}/{rest_forward}"),
                    &FORWARD,
                    Shells::PowerShell,
                ));
            }
            match *name {
                "HOME" => {
                    leads.push((format!("$HOME/{rest_forward}"), &FORWARD, Shells::Both));
                    leads.push((format!("${{HOME}}/{rest_forward}"), &FORWARD, Shells::Bash));
                    leads.push((format!("~/{rest_forward}"), &FORWARD, Shells::Both));
                    // Claude Code puts its `HOME` in place of a Bash `$HOME` the shell would
                    // expand before it matches (`$HOME/x` is `C:\Users\asha/x`), so the
                    // literal `$HOME/…` matches only where the shell leaves it alone.
                    leads.push((format!("{value}/{rest_forward}"), &FORWARD, Shells::Bash));
                }
                // Its home folder, should Claude Code take it from there.
                "USERPROFILE" => {
                    leads.push((format!("{value}/{rest_forward}"), &FORWARD, Shells::Bash));
                }
                _ => {
                    leads.push((format!("${name}/{rest_forward}"), &FORWARD, Shells::Bash));
                    leads.push((
                        format!("${{{name}}}/{rest_forward}"),
                        &FORWARD,
                        Shells::Bash,
                    ));
                }
            }
            for (lead, junctions, shells) in leads {
                push(lead, junctions, shells);
            }
        }
    }
    forms
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
