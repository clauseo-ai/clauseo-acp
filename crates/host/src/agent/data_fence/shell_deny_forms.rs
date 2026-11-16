//! The ways a command writes the data folder (`super`'s module doc): its folder name alone when
//! that holds Clauseo's name (`ai.clauseo.desktop`, a configured `Clauseo Data`, or the 8.3
//! short form of either, `AICLAU~1.DES`), or a configured folder's path in each spelling and under each of the variables a
//! command may write its parent with, each with the separators that may follow it and the
//! shells that can write it.

use std::ffi::OsString;

use super::JUNCTIONS;
use crate::agent::data_fence::LaunchDeny;
use crate::network_paths::fold_name;

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
    /// Whether `lead` is the folder's name alone ([`alone_name`]), which ends every other way
    /// of writing it, the escaped form's too.
    pub(super) alone: bool,
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

/// The ways a command writes each of `deny`'s spellings of `<data>`: when its last name holds
/// Clauseo's ([`alone_name`]), that name alone, which every other way of writing it ends in
/// (one rule set in place of four, and a mixed spelling such as `%LOCALAPPDATA%\AICLAU~1.DES`
/// is caught too); otherwise its path with `\`, with `/`,
/// with `\\` and as Git Bash's `/c/…`, and under a folder one of [`VARS`] names (read with `var`,
/// in any case), `%VAR%` (and `%var%`), `$env:VAR`, `$VAR` and `${VAR}`, and `~` for the home,
/// followed by the rest. A spelling that is not UTF-8 gets none.
pub(super) fn forms(
    deny: &LaunchDeny,
    var: &dyn Fn(&str) -> Option<OsString>,
    variables: bool,
) -> Vec<Form> {
    let mut forms: Vec<Form> = Vec::new();
    let mut push =
        |lead: String, alone: bool, junctions: &'static [&'static str], shells: Shells| {
            if !lead.is_empty() && !forms.iter().any(|form| form.lead == lead) {
                forms.push(Form {
                    lead,
                    alone,
                    junctions,
                    shells,
                });
            }
        };
    let names: Vec<String> = deny
        .spellings
        .iter()
        .filter_map(|spelling| Some(last_name(&drive_form(spelling.to_str()?)).to_owned()))
        .collect();
    let own: Vec<String> = deny
        .matters
        .own
        .iter()
        .filter_map(|place| Some(fold_name(place.to_str()?)))
        .collect();
    for spelling in &deny.spellings {
        let Some(text) = spelling.to_str() else {
            continue;
        };
        let back = drive_form(text);
        let back = back.trim_end_matches('\\');
        let name = last_name(back);
        if alone_name(name, &names, &own) {
            push(name.to_owned(), true, &JUNCTIONS, Shells::Both);
            continue;
        }
        let forward = back.replace('\\', "/");
        push(back.to_owned(), false, &JUNCTIONS, Shells::Both);
        push(forward.clone(), false, &FORWARD, Shells::Both);
        // A doubled `\` is Bash's escape, and Win32 reads it as one `\` in any shell.
        push(back.replace('\\', "\\\\"), false, &JUNCTIONS, Shells::Both);
        let mut chars = forward.chars();
        if let (Some(drive), Some(':')) = (chars.next(), chars.next())
            && drive.is_ascii_alphabetic()
        {
            let git = format!("/{}{}", drive.to_ascii_lowercase(), chars.as_str());
            push(git, false, &FORWARD, Shells::Bash);
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
                    // PowerShell reads `${HOME}` as its `$HOME`, braced.
                    leads.push((format!("${{HOME}}/{rest_forward}"), &FORWARD, Shells::Both));
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
                push(lead, false, junctions, shells);
            }
        }
    }
    forms
}

/// `path`'s last name (`\\`-separated, a trailing one dropped), split by hand: a Windows path is
/// one name to `Path` off Windows.
fn last_name(path: &str) -> &str {
    let path = path.trim_end_matches('\\');
    path.rsplit('\\').next().unwrap_or(path)
}

/// Whether a spelling of `<data>` whose last name is `name` is written as that name alone: it
/// is Clauseo's own (`clauseo_settings::IDENTIFIER`), or it holds `clauseo` in any case
/// (`Clauseo Data`), or it is the 8.3 short form ([`is_short_of`]) of such a name among
/// `names`, the last names of the launch's spellings (`AICLAU~1.DES` beside
/// `ai.clauseo.desktop`). A generic name (`FirmData`) never is: a command naming it alone may
/// mean another folder. Nor, but for Clauseo's own name, is a name that the session's own matter
/// folder holds (`own`, each spelling folded), which its rules would deny with it. What it
/// costs: any command naming the name, whatever folder it means, is denied
/// (`D:\Clauseo Data Export` beside a data folder `Clauseo Data`).
pub(super) fn alone_name(name: &str, names: &[String], own: &[String]) -> bool {
    // The default, `ai.clauseo.desktop` (a reverse-DNS app id), starts no name a lawyer's own
    // file or folder carries, so its rules over-match nothing real.
    if name == clauseo_settings::IDENTIFIER {
        return true;
    }
    let clauseo = |text: &str| fold_name(text).contains("clauseo");
    let ours = clauseo(name)
        || names
            .iter()
            .any(|long| clauseo(long) && is_short_of(name, long));
    let folded = fold_name(name);
    ours && !folded.is_empty() && !own.iter().any(|place| place.contains(&folded))
}

/// Whether `short` is an 8.3 short name Windows may give `long`: `STEM~N` with an extension of
/// at most three characters, the stem a start of `long`'s name before its last dot with spaces
/// and dots dropped (`AICLAU` of `ai.clauseo`), or its first two characters and four hex
/// digits (the form Windows hashes to past the fourth like name), the extension the start of
/// `long`'s after it (`DES` of `desktop`); compared in any ASCII case.
pub(super) fn is_short_of(short: &str, long: &str) -> bool {
    let (base, ext) = short.split_once('.').unwrap_or((short, ""));
    if ext.contains('.') || ext.len() > 3 || base.len() > 8 {
        return false;
    }
    let Some((stem, number)) = base.rsplit_once('~') else {
        return false;
    };
    if stem.is_empty() || number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let (long_base, long_ext) = match long.rfind('.') {
        Some(at) if at > 0 => (
            long.get(..at).unwrap_or(long),
            long.get(at + 1..).unwrap_or(""),
        ),
        _ => (long, ""),
    };
    let squeeze = |text: &str| -> String {
        text.chars()
            .filter(|c| !matches!(c, ' ' | '.'))
            .map(|c| {
                if "+,;=[]".contains(c) {
                    '_'
                } else {
                    c.to_ascii_uppercase()
                }
            })
            .collect()
    };
    let (long_base, long_ext) = (squeeze(long_base), squeeze(long_ext));
    let stem = stem.to_ascii_uppercase();
    let hashed = stem.len() == 6
        && stem
            .get(2..)
            .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))
        && stem
            .get(..2)
            .is_some_and(|start| long_base.starts_with(start));
    let ext_ok = long_ext.chars().take(3).collect::<String>() == ext.to_ascii_uppercase();
    (long_base.starts_with(&stem) || hashed) && ext_ok
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
