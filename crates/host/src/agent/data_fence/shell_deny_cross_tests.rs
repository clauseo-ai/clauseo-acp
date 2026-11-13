//! The Windows shell deny rules against generated spellings (stage C's cross-product): for each
//! protected target, separator runs and `.\` steps between names, trailing dots and spaces,
//! streams, quoting, letter case and every way of writing the data folder are denied; the own
//! folders written with the same variety pass, and so do the Windows VM's commands. And against
//! the rules before #1035 (`old_rules`, #999's `*<data>*`): whatever they denied of what is not
//! the session's own, these deny too.

use std::ffi::OsString;
use std::path::PathBuf;

use super::tests::{Rule, prepare};
use super::windows_shell_deny_rules;
use crate::agent::data_fence::LaunchDeny;
use crate::agent::data_fence::big_folder::{self, OWN_RUN, PREFIX_RUN, matter_id};
use crate::pipeline::tests::support::TempDir;

/// The default data folder of the VM's lawyer.
const DATA: &str = r"C:\Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop";

/// A configured data folder (stage C's example).
const CONFIGURED: &str = r"D:\FirmData\Clauseo";

pub(super) fn env(name: &str) -> Option<OsString> {
    match name {
        "LOCALAPPDATA" => Some(r"C:\Users\clauseo-lawyer\AppData\Local".into()),
        "APPDATA" => Some(r"C:\Users\clauseo-lawyer\AppData\Roaming".into()),
        "USERPROFILE" | "HOME" => Some(r"C:\Users\clauseo-lawyer".into()),
        _ => None,
    }
}

/// Rules compiled once for [`Rules::denies`], as Claude Code reads them ([`super::tests::Rule`]).
pub(super) struct Rules {
    bash: Vec<Rule>,
    powershell: Vec<Rule>,
}

impl Rules {
    pub(super) fn new(rules: &[String]) -> Self {
        let (powershell, bash): (Vec<Rule>, Vec<Rule>) = rules
            .iter()
            .filter_map(|rule| Rule::new(rule))
            .partition(|rule| rule.tool() == "PowerShell");
        Self { bash, powershell }
    }

    /// Whether a rule of `tool` matches `command` (PowerShell's in any case).
    pub(super) fn denies(&self, tool: &str, command: &str) -> bool {
        let powershell = tool == "PowerShell";
        let rules = if powershell {
            &self.powershell
        } else {
            &self.bash
        };
        let text = prepare(command, powershell);
        rules.iter().any(|rule| rule.hits_prepared(&text))
    }
}

/// A deterministic stream of numbers (a linear congruential generator).
struct Lcg(u64);

impl Lcg {
    fn pick(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from((self.0 >> 33) % u64::try_from(n).unwrap()).unwrap()
    }
}

/// The separator runs a command may write between two names.
const RUNS: [&str; 6] = ["\\", "/", "\\\\", "//", "\\/", "/\\"];

/// A step that changes nothing, or none.
const STEPS: [&str; 3] = ["", ".\\", "./"];

/// What Win32 drops from a name's end, or nothing.
const TRAILS: [&str; 4] = ["", ".", " ", "..."];

/// A stream after the last name, or nothing.
const STREAMS: [&str; 3] = ["", "::$DATA", ":x"];

/// The ways of writing the default data folder: each drive, share and device form, the
/// variables in either case, the home forms, the bare name, and its name with a trailing dot
/// or space (`trailing`).
pub(super) fn default_leads(trailing: bool) -> Vec<String> {
    let rest = r"Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop";
    let forward = rest.replace('\\', "/");
    let mut leads = vec![
        format!(r"C:\{rest}"),
        format!("C:/{forward}"),
        format!(r"C:\\{}", rest.replace('\\', r"\\")),
        format!("/c/{forward}"),
        format!(r"\\?\C:\{rest}"),
        format!(r"\\localhost\C$\{rest}"),
        format!(r"\\127.0.0.1\c$\{rest}"),
        format!(r"\\?\UNC\localhost\c$\{rest}"),
        format!(r"\\.\C:\{rest}"),
        r"%LOCALAPPDATA%\ai.clauseo.desktop".to_owned(),
        r"%localappdata%\ai.clauseo.desktop".to_owned(),
        r"$env:LOCALAPPDATA\ai.clauseo.desktop".to_owned(),
        "$LOCALAPPDATA/ai.clauseo.desktop".to_owned(),
        "${LOCALAPPDATA}/ai.clauseo.desktop".to_owned(),
        r"%USERPROFILE%\AppData\Local\ai.clauseo.desktop".to_owned(),
        r"~\AppData\Local\ai.clauseo.desktop".to_owned(),
        "$HOME/AppData/Local/ai.clauseo.desktop".to_owned(),
        r"$env:USERPROFILE\AppData\Local\ai.clauseo.desktop".to_owned(),
        "ai.clauseo.desktop".to_owned(),
    ];
    if trailing {
        for trail in [".", " ", "..."] {
            leads.push(format!(r"C:\{rest}{trail}"));
        }
    }
    leads
}

/// The configured folder's spellings the rules deny: its path with `\`, `/` and `\\` (a doubled
/// separator throughout), Git Bash's `/d/…`, and its name with a trailing dot.
fn configured_leads() -> Vec<String> {
    vec![
        CONFIGURED.to_owned(),
        CONFIGURED.replace('\\', "/"),
        CONFIGURED.replace('\\', r"\\"),
        "/d/FirmData/Clauseo".to_owned(),
        format!("{CONFIGURED}."),
    ]
}

/// The protected places of the big folder (`big_folder.rs`), as names under `<data>`.
fn protected() -> Vec<Vec<String>> {
    let names = |path: &str| path.split('\\').map(str::to_owned).collect::<Vec<_>>();
    vec![
        names(r"agents\claude-acp\home\.credentials.json"),
        names(r"agents\codex-acp\home\auth.json"),
        names(r"tools\x"),
        names(r"agents\runtime\clauseo-python-3.13.18-c2-windows-x64\python.exe"),
        names(r"profile\claude-plugin\skills\x\SKILL.md"),
        names(&format!(r"agent-state\{}\notes\x", matter_id(7))),
        names(&format!(r"run\{PREFIX_RUN}\work\x")),
        names(r"run\01a1d5c4-90d5-7548-96d0-4a1610744922-2\x"),
        names(clauseo_settings::SETTINGS_FILE),
        names("state.sqlite"),
        names(r"logs\host.log"),
        names(r"launch-settings\x.json"),
        names(r"matters\x"),
    ]
}

/// `names` after `lead`, the separators and steps between them and what follows each name
/// picked by `lcg`, or by `uniform` (one run everywhere, nothing else) when given.
fn spelled(lead: &str, names: &[String], lcg: &mut Lcg, uniform: Option<&str>) -> String {
    let mut out = lead.to_owned();
    for (n, name) in names.iter().enumerate() {
        if let Some(run) = uniform {
            out.push_str(run);
        } else {
            out.push_str(RUNS[lcg.pick(RUNS.len())]);
            out.push_str(STEPS[lcg.pick(STEPS.len())]);
        }
        out.push_str(name);
        if uniform.is_none() {
            out.push_str(TRAILS[lcg.pick(TRAILS.len())]);
            if n + 1 == names.len() {
                out.push_str(STREAMS[lcg.pick(STREAMS.len())]);
            }
        }
    }
    out
}

/// A command naming `path`, quoted as `n` picks, and the tools it is checked in: both, or for
/// an upper-case copy PowerShell alone (Bash rules match as written, the residual).
fn commands(path: &str, n: usize) -> Vec<(&'static str, String)> {
    let quoted = match n % 3 {
        0 => path.to_owned(),
        1 => format!("\"{path}\""),
        _ => format!("'{path}'"),
    };
    let command = format!("type {quoted}");
    let mut out = vec![("Bash", command.clone()), ("PowerShell", command.clone())];
    if n.is_multiple_of(4) {
        out.push(("PowerShell", command.to_uppercase()));
    }
    out
}

/// Every generated spelling of each protected place, after each of `leads`: one per separator
/// run used throughout, then `samples` with runs, steps, trailing dots and spaces and streams
/// picked deterministically. With the tools each is checked in.
fn generated(leads: &[String], samples: usize) -> Vec<(&'static str, String)> {
    generated_over(leads, &protected(), samples)
}

/// [`generated`] for `places`.
pub(super) fn generated_over(
    leads: &[String],
    places: &[Vec<String>],
    samples: usize,
) -> Vec<(&'static str, String)> {
    let mut lcg = Lcg(0x5eed);
    let mut out = Vec::new();
    for names in places {
        for lead in leads {
            let mut paths: Vec<String> = RUNS
                .iter()
                .map(|run| spelled(lead, names, &mut lcg, Some(run)))
                .collect();
            for _ in 0..samples {
                paths.push(spelled(lead, names, &mut lcg, None));
            }
            for path in paths {
                let n = lcg.pick(12);
                out.extend(commands(&path, n));
            }
        }
    }
    out
}

/// The own folders, written after `lead` (whose own separator is `sep`) with the variety inside
/// them: separator runs and `.\` steps between their names.
fn own_commands(lead: &str, sep: &str, samples: usize) -> Vec<(&'static str, String)> {
    let key = super::super::claude_project_key(&big_folder::matter_folder(0));
    let own = own_folders(OWN_RUN, &matter_id(0), &key);
    own_commands_over(lead, sep, samples, &own)
}

/// The own folders of a session with run folder `run`, matter `matter` and Claude project
/// `key`: each as the names down to it and names inside it.
pub(super) fn own_folders(run: &str, matter: &str, key: &str) -> Vec<(Vec<String>, Vec<String>)> {
    let names = |list: &[&str]| {
        list.iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    vec![
        (names(&["run", run]), names(&["work", "reply.md"])),
        (
            names(&["agent-state", matter, "notes"]),
            names(&["sub", "notes.md"]),
        ),
        (
            names(&["agents", "claude-acp", "home", "plans"]),
            names(&["plan.md"]),
        ),
        (
            names(&[
                "agents",
                "claude-acp",
                "home",
                "projects",
                key,
                "0193-session",
            ]),
            names(&["tool-results", "r.txt"]),
        ),
    ]
}

/// [`own_commands`] for `own` ([`own_folders`]).
pub(super) fn own_commands_over(
    lead: &str,
    sep: &str,
    samples: usize,
    own: &[(Vec<String>, Vec<String>)],
) -> Vec<(&'static str, String)> {
    let mut lcg = Lcg(0x0b5e);
    let mut out = Vec::new();
    for (above, inside) in own {
        let folder = above.join(sep);
        for _ in 0..samples {
            let mut path = format!("{lead}{sep}{folder}");
            for name in inside {
                path.push_str(RUNS[lcg.pick(RUNS.len())]);
                path.push_str(STEPS[lcg.pick(STEPS.len())]);
                path.push_str(name);
            }
            let n = lcg.pick(12);
            out.extend(commands(&path, n));
            let matter = r"C:\Users\clauseo-lawyer\CloudProxy\Matter 00\x.docx";
            let pandoc = format!(r#"pandoc "{path}" -o "{matter}""#);
            out.push(("Bash", pandoc.clone()));
            out.push(("PowerShell", pandoc));
        }
    }
    out
}

/// The own separator of a lead: `/` for the forward forms, `\\` for the escaped one, else `\`.
pub(super) fn lead_sep(lead: &str) -> &'static str {
    if lead.starts_with(r"C:\\") {
        r"\\"
    } else if lead.contains('/') || lead.starts_with("$LOCALAPPDATA") {
        "/"
    } else {
        "\\"
    }
}

/// The launch of the big folder's session with `<data>` spelled `data`, and its rules.
fn launch(root: &std::path::Path, data: &str) -> (LaunchDeny, Rules) {
    let big = big_folder::big(root);
    let deny = LaunchDeny {
        spellings: vec![PathBuf::from(data)],
        ..big.deny
    };
    let rules = Rules::new(&windows_shell_deny_rules(&deny, &env));
    (deny, rules)
}

/// The Windows VM's 7 denied commands (`shell_deny_launch_tests.rs` has them in full), in the
/// big folder's terms: this matter's notes, the own run's work folder.
fn vm_commands() -> Vec<(&'static str, String)> {
    let base = DATA.replace('\\', "/");
    let notes = format!("{base}/agent-state/{}/notes", matter_id(0));
    let work = format!("{base}/run/{OWN_RUN}/work");
    let work_back = work.replace('/', "\\");
    let notes_back = notes.replace('/', "\\");
    vec![
        (
            "Bash",
            format!(r#"ls "{notes}"; ls; pandoc "Notes.docx" -t plain"#),
        ),
        (
            "PowerShell",
            format!(r#"Get-ChildItem "{notes_back}"; Get-ChildItem; stetdoc skills get core"#),
        ),
        (
            "Bash",
            format!(r#"ls -la "{notes}"; echo ---; ls -la "{work}""#),
        ),
        (
            "PowerShell",
            format!(
                r#"pandoc "{work_back}/reply.md" -o "Reply to legal notice.docx"; if ($?) {{ Remove-Item "{work_back}\reply.md" }}"#
            ),
        ),
        (
            "PowerShell",
            format!(r#"pandoc "{work_back}/reply.md" -o "Reply to legal notice.docx""#),
        ),
        (
            "Bash",
            format!(r#"head -100; echo ---; ls -la "{notes}"; echo ---; ls -la "{work}""#),
        ),
    ]
}

/// The default folder: every generated spelling of every protected place is denied in the
/// tools it applies to; the own folders written with the same variety, and the VM's commands,
/// pass.
#[test]
fn every_generated_spelling_of_a_protected_place_is_denied() {
    let dir = TempDir::new("shell-deny-cross");
    let (_, rules) = launch(dir.path(), DATA);
    let denied = generated(&default_leads(true), 6);
    for (tool, command) in &denied {
        assert!(rules.denies(tool, command), "{tool} passes: {command:?}");
    }
    let mut passed = vm_commands();
    for lead in default_leads(false) {
        passed.extend(own_commands(&lead, lead_sep(&lead), 3));
    }
    for (tool, command) in &passed {
        assert!(!rules.denies(tool, command), "{tool} denies: {command:?}");
    }
    eprintln!(
        "cross-product: {} denied spellings, {} own commands passed",
        denied.len(),
        passed.len()
    );
}

/// A configured folder (`D:\FirmData\Clauseo`): its spellings the rules know are denied for
/// every protected place, stage C's examples among them; a folder whose name only starts the
/// same stays open; the accepted residuals still pass, so a change that closes or opens one
/// shows here.
#[test]
fn a_configured_folders_spellings_are_denied_and_its_residuals_listed() {
    let dir = TempDir::new("shell-deny-cross-configured");
    let (_, rules) = launch(dir.path(), CONFIGURED);
    let mut denied = generated(&configured_leads(), 4);
    for example in [
        r"type D:\FirmData\Clauseo/settings.json",
        r"type D:\FirmData\Clauseo/state.sqlite",
        r"type D:\FirmData\Clauseo/logs\host.log",
        r"type D:\FirmData\Clauseo/launch-settings\own.json",
        r"type D:\FirmData\Clauseo/run\01a1d5be-5efa-73f6-a24b-dc90c01883d9-10\relay-token",
        r"type D:\FirmData\Clauseo/matters\x",
        r"copy /y evil.exe D:\FirmData\Clauseo/tools\agent-browser\agent-browser",
        r"type D:\FirmData\Clauseo.\settings.json",
        r"type D:\FirmData\Clauseo \settings.json",
        r"type D:\FirmData\Clauseo\settings.json::$DATA",
        r"type \\?\D:\FirmData\Clauseo\settings.json",
        r"type \\.\D:\FirmData\Clauseo\settings.json",
    ] {
        denied.push(("Bash", example.to_owned()));
        denied.push(("PowerShell", example.to_owned()));
    }
    // Git Bash's `/d/…` is Bash's alone: PowerShell reads it as a folder of the current drive.
    denied.retain(|(tool, command)| *tool == "Bash" || !command.to_lowercase().contains("/d/"));
    for (tool, command) in &denied {
        assert!(rules.denies(tool, command), "{tool} passes: {command:?}");
    }
    let mut passed = own_commands(CONFIGURED, "\\", 3);
    passed.extend(own_commands(&CONFIGURED.replace('\\', r"\\"), r"\\", 2));
    passed.push(("PowerShell", r"type D:\FirmData\ClauseoBackup\x".to_owned()));
    passed.push(("Bash", r"type D:\FirmData\ClauseoBackup\x".to_owned()));
    // The accepted residuals (decision 0030): the configured path written other than whole, a
    // share path to it, a variable naming part of it, Bash's letter case. (A device path,
    // `\\?\D:\…`, holds the drive path and is denied above.)
    for residual in [
        r"type D:\FirmData\Other\..\Clauseo\settings.json",
        r"type D:\FirmData\.\Clauseo\settings.json",
        r"type D:\FirmData/Clauseo\settings.json",
        r"type \\localhost\D$\FirmData\Clauseo\settings.json",
        r"type %FIRMDATA%\Clauseo\settings.json",
    ] {
        passed.push(("PowerShell", residual.to_owned()));
        passed.push(("Bash", residual.to_owned()));
    }
    passed.push(("Bash", r"type d:\firmdata\clauseo\settings.json".to_owned()));
    for (tool, command) in &passed {
        assert!(!rules.denies(tool, command), "{tool} denies: {command:?}");
    }
}

/// The rules before #1035 (#999, `shell_deny.rs` at b386fbd50): `*<form>*` for each way of
/// writing `<data>` and for Clauseo's folder name alone, kept here for the differential test.
fn old_rules(spellings: &[PathBuf], var: &dyn Fn(&str) -> Option<OsString>) -> Vec<String> {
    let mut forms: Vec<String> = Vec::new();
    let mut push = |form: String| {
        if !form.is_empty() && !forms.contains(&form) {
            forms.push(form);
        }
    };
    for spelling in spellings {
        let Some(text) = spelling.to_str() else {
            continue;
        };
        let back = text.trim_end_matches('\\');
        let forward = back.replace('\\', "/");
        push(back.to_owned());
        push(forward.clone());
        push(back.replace('\\', r"\\"));
        let mut chars = forward.chars();
        if let (Some(drive), Some(':')) = (chars.next(), chars.next()) {
            push(format!("/{}{}", drive.to_ascii_lowercase(), chars.as_str()));
        }
        for name in ["LOCALAPPDATA", "APPDATA"] {
            let Some(value) = var(name).and_then(|v| v.into_string().ok()) else {
                continue;
            };
            let value = value.trim_end_matches('\\');
            let folded = back.to_ascii_lowercase();
            let Some(rest) = folded
                .strip_prefix(&value.to_ascii_lowercase())
                .and_then(|rest| rest.strip_prefix('\\'))
                .and_then(|rest| back.get(back.len() - rest.len()..))
            else {
                continue;
            };
            let rest_forward = rest.replace('\\', "/");
            for lead in [format!("%{name}%"), format!("$env:{name}")] {
                push(format!("{lead}\\{rest}"));
                push(format!("{lead}/{rest_forward}"));
            }
            for lead in [format!("${name}"), format!("${{{name}}}")] {
                push(format!("{lead}/{rest_forward}"));
            }
        }
        if back.rsplit('\\').next() == Some(clauseo_settings::IDENTIFIER) {
            push(clauseo_settings::IDENTIFIER.to_owned());
        }
    }
    let escape = |text: &str| text.replace('\\', r"\\").replace('*', r"\*");
    let mut rules = Vec::new();
    for tool in ["Bash", "PowerShell"] {
        for form in &forms {
            rules.push(format!("{tool}(*{}*)", escape(form)));
        }
    }
    rules
}

/// Stage C's differential probe: of every generated spelling of what is not the session's own
/// (the protected places, the other runs and matters, the folders above the own ones named
/// alone), whatever the old rules denied the new ones deny; the own folders pass the new ones.
#[test]
fn whatever_the_old_rules_denied_of_the_rest_the_new_ones_deny() {
    let dir = TempDir::new("shell-deny-differential");
    let (_, rules) = launch(dir.path(), DATA);
    let old = Rules::new(&old_rules(&[PathBuf::from(DATA)], &env));
    let mut others = generated(&default_leads(true), 3);
    for lead in default_leads(true) {
        let sep = lead_sep(&lead);
        for place in [
            String::new(),
            "run".to_owned(),
            "agent-state".to_owned(),
            format!("run{sep}{PREFIX_RUN}"),
            format!("agent-state{sep}{}", matter_id(9)),
            format!("run{sep}*"),
            format!("run{sep}{OWN_RUN}{sep}..{sep}{PREFIX_RUN}"),
            format!("agent-state{sep}{}{sep}notes{sep}..{sep}..", matter_id(0)),
        ] {
            let path = if place.is_empty() {
                lead.clone()
            } else {
                format!("{lead}{sep}{place}")
            };
            for n in 0..3 {
                others.extend(commands(&path, n));
            }
        }
    }
    let mut checked = 0;
    for (tool, command) in &others {
        if old.denies(tool, command) {
            checked += 1;
            assert!(
                rules.denies(tool, command),
                "{tool}: old denies, new passes: {command:?}"
            );
        }
    }
    assert!(checked > others.len() / 2, "{checked} of {}", others.len());
    let mut own = vm_commands();
    for lead in default_leads(false) {
        own.extend(own_commands(&lead, lead_sep(&lead), 2));
    }
    for (tool, command) in &own {
        assert!(!rules.denies(tool, command), "{tool} denies: {command:?}");
    }
}
