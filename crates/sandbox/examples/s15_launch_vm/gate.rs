//! The UI-limit gate, decided per effect ([`gate`]). The effects are `desktop-create`,
//! `switch-desktop` and `clipboard-write`, which the agent's `probe job-ui` prints under
//! `ui-effect` and each control's `probe ui-control` under `ui-control`. An effect is
//! *discriminating* when a control that counts succeeded at it: variant (g) alone when it ran
//! with its own shape, else any of (a) to (d) that ran with theirs (`checks_ctl.rs`). Then:
//!
//! - FAIL when the agent's child succeeded at a counted effect (the limit is missing), or was
//!   refused at a discriminating one with a code the limit doesn't give;
//! - PASS when it was refused at a discriminating effect (strict mode: at each of the three);
//! - INFO when no effect is discriminating (strict mode: FAIL).
//!
//! `switch-desktop` counts only in strict mode (`--interactive-desktop`); otherwise it is
//! information. An effect no control could carry is named in the row text as information.

/// The calls of a clipboard write whose failure is the job's `WRITECLIPBOARD` limit (the owner
/// window and the memory block come before or beside them and prove nothing).
const CLIPBOARD_CALLS: [&str; 3] = ["OpenClipboard", "EmptyClipboard", "SetClipboardData"];
/// `ERROR_ACCESS_DENIED`, and the same as an `HRESULT`.
const ACCESS_DENIED: [u32; 2] = [5, 0x8007_0005];

/// One effect of the UI limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Effect {
    /// `CreateDesktopW` on the caller's own station (the `DESKTOP` limit).
    Desktop,
    /// `SwitchDesktop` to a desktop it made (the `DESKTOP` limit).
    Switch,
    /// A clipboard write (the `WRITECLIPBOARD` limit).
    Clipboard,
}

/// The effects, in the order the probes print them.
pub(crate) const EFFECTS: [Effect; 3] = [Effect::Desktop, Effect::Switch, Effect::Clipboard];

impl Effect {
    /// The key both probes print it under.
    fn key(self) -> &'static str {
        match self {
            Effect::Desktop => "desktop-create",
            Effect::Switch => "switch-desktop",
            Effect::Clipboard => "clipboard-write",
        }
    }

    /// Its name in the row text.
    fn label(self) -> &'static str {
        match self {
            Effect::Desktop => "desktop",
            Effect::Switch => "switch-desktop",
            Effect::Clipboard => "clipboard",
        }
    }

    /// What a control did when it succeeded, for the row text.
    fn verb(self) -> &'static str {
        match self {
            Effect::Desktop => "create one",
            Effect::Switch => "switch to one",
            Effect::Clipboard => "write it",
        }
    }
}

/// What one side got at one effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Got {
    /// It succeeded.
    Ok,
    /// It was refused with what the limit gives: 5 (for the clipboard, from one of
    /// [`CLIPBOARD_CALLS`]; for the switch, a create refused with 5 counts too). The value.
    Refused(String),
    /// Anything else, or no line: the value.
    Failed(String),
}

impl Got {
    fn text(&self) -> &str {
        match self {
            Got::Ok => "ok",
            Got::Refused(value) | Got::Failed(value) => value,
        }
    }
}

/// The three effects' results from the lines `<prefix> <key> <value>` in `lines`; `prefix` is
/// `ui-effect` (the agent's) or `ui-control` (a control's).
pub(crate) fn read(lines: &[String], prefix: &str) -> [Got; 3] {
    EFFECTS.map(|effect| read_one(lines, prefix, effect))
}

fn read_one(lines: &[String], prefix: &str, effect: Effect) -> Got {
    let key = format!("{prefix} {} ", effect.key());
    let Some(value) = lines
        .iter()
        .find_map(|l| l.strip_prefix(key.as_str()))
        .map(str::trim)
    else {
        return Got::Failed("none printed".to_owned());
    };
    let (ok, refused) = match effect {
        Effect::Desktop => (value == "0", value == "5"),
        Effect::Switch => (value == "0", value == "5" || value == "create-failed 5"),
        Effect::Clipboard => (value == "ok", clipboard_refused(value)),
    };
    match (ok, refused) {
        (true, _) => Got::Ok,
        (false, true) => Got::Refused(value.to_owned()),
        (false, false) => Got::Failed(value.to_owned()),
    }
}

/// Whether the clipboard value is `failed <call> <code>` with the limit's call and code.
fn clipboard_refused(value: &str) -> bool {
    let Some((call, code)) = value
        .strip_prefix("failed ")
        .and_then(|rest| rest.split_once(' '))
    else {
        return false;
    };
    let code = match code.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => code.parse().ok(),
    };
    CLIPBOARD_CALLS.contains(&call) && code.is_some_and(|c| ACCESS_DENIED.contains(&c))
}

/// How close a control variant is to the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rank {
    /// (g): the agent's token on the agent's own station, outside the job.
    Agent,
    /// (a) to (d): can carry an effect when (g) didn't run.
    Close,
    /// (e) and (f): evidence only.
    Far,
}

/// One control variant's run.
#[derive(Debug, Clone)]
pub(crate) struct Control {
    /// `a` to `g`.
    pub(crate) key: char,
    pub(crate) rank: Rank,
    /// Whether it ran on its station with its token shape (`checks_ctl::shape`); why not.
    pub(crate) shape: Result<(), String>,
    /// Its three results.
    pub(crate) got: [Got; 3],
}

/// The gate's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Pass,
    Fail,
    Info,
}

/// [`gate`]'s result.
#[derive(Debug)]
pub(crate) struct Judged {
    pub(crate) kind: Kind,
    /// One part per effect, `; `-joined.
    pub(crate) text: String,
    /// The variants that carried a discriminating effect, each once.
    pub(crate) carriers: Vec<char>,
}

/// How one effect came out.
#[derive(PartialEq, Eq)]
enum Status {
    Proven,
    Broken,
    Open,
}

/// The gate (the module's rules) on the agent's results and the controls'.
pub(crate) fn gate(agent: &[Got; 3], controls: &[Control], strict: bool) -> Judged {
    let g = controls
        .iter()
        .find(|c| c.rank == Rank::Agent && c.shape.is_ok());
    let mut parts = Vec::new();
    let mut carriers = Vec::new();
    let mut statuses = Vec::new();
    for (at, effect) in EFFECTS.into_iter().enumerate() {
        let worked = |c: &&Control| c.shape.is_ok() && c.got.get(at) == Some(&Got::Ok);
        let carry: Vec<char> = match g {
            Some(g) => std::iter::once(g).filter(worked).map(|c| c.key).collect(),
            None => controls
                .iter()
                .filter(|c| c.rank == Rank::Close)
                .filter(worked)
                .map(|c| c.key)
                .collect(),
        };
        let evidence = keys(
            controls
                .iter()
                .filter(worked)
                .map(|c| c.key)
                .filter(|k| !carry.contains(k)),
        );
        let codes = controls
            .iter()
            .filter(|c| c.shape.is_ok() && !worked(c))
            .map(|c| format!("({}) {}", c.key, c.got.get(at).map_or("none", Got::text)))
            .collect::<Vec<_>>()
            .join(", ");
        let codes = if codes.is_empty() {
            "none ran".to_owned()
        } else {
            codes
        };
        let mine = agent
            .get(at)
            .cloned()
            .unwrap_or(Got::Failed("none".to_owned()));
        let (label, by) = (effect.label(), keys(carry.iter().copied()));
        let (status, mut part) = match (carry.is_empty(), &mine) {
            (true, Got::Ok) => (
                Status::Broken,
                format!(
                    "{label}: the agent's child succeeded, so the limit is missing (no control \
                     could {}: {codes})",
                    effect.verb()
                ),
            ),
            (false, Got::Ok) => (
                Status::Broken,
                format!("{label}: the agent's child succeeded, as {by} did: the limit is missing"),
            ),
            (false, Got::Refused(code)) => (
                Status::Proven,
                format!(
                    "{label}: discriminating, carried by {by}; the agent's child was refused \
                         ({code})"
                ),
            ),
            (false, Got::Failed(code)) => (
                Status::Broken,
                format!(
                    "{label}: carried by {by}, but the agent's child gave {code}, not the \
                         limit's refusal"
                ),
            ),
            (true, other) => (
                Status::Open,
                format!(
                    "{label}: no control could {} ({codes}), information only; the agent's \
                     child gave {}",
                    effect.verb(),
                    other.text()
                ),
            ),
        };
        if !evidence.is_empty() {
            part = format!("{part}; {evidence} also did, evidence only");
        }
        if strict || effect != Effect::Switch {
            if status == Status::Proven {
                for key in &carry {
                    if !carriers.contains(key) {
                        carriers.push(*key);
                    }
                }
            }
            statuses.push(status);
        } else {
            part.push_str(" (information, never counted without --interactive-desktop)");
        }
        parts.push(part);
    }
    let proven = statuses.iter().filter(|s| **s == Status::Proven).count();
    let kind = if statuses.contains(&Status::Broken) {
        Kind::Fail
    } else if strict {
        if proven == EFFECTS.len() {
            Kind::Pass
        } else {
            Kind::Fail
        }
    } else if proven > 0 {
        Kind::Pass
    } else {
        Kind::Info
    };
    let lead = if strict {
        "--interactive-desktop: each of desktop-create, SwitchDesktop and clipboard-write needs a \
         discriminating control that the agent's child fails; "
    } else {
        ""
    };
    Judged {
        kind,
        text: format!("{lead}{}", parts.join("; ")),
        carriers,
    }
}

/// `(a)`, `(a), (b)`…, or empty.
fn keys(keys: impl Iterator<Item = char>) -> String {
    keys.map(|k| format!("({k})"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::{Control, Got, Kind, Rank, gate, read};

    fn no(code: &str) -> Got {
        Got::Failed(code.to_owned())
    }

    fn refused(code: &str) -> Got {
        Got::Refused(code.to_owned())
    }

    fn control(key: char, rank: Rank, got: [Got; 3]) -> Control {
        Control {
            key,
            rank,
            shape: Ok(()),
            got,
        }
    }

    /// The agent as CI sees it: refused at every effect.
    fn agent() -> [Got; 3] {
        [
            refused("5"),
            refused("create-failed 5"),
            refused("failed SetClipboardData 5"),
        ]
    }

    #[test]
    fn a_clipboard_only_control_with_the_agent_refused_passes() {
        let a = control('a', Rank::Close, [no("8"), no("create-failed 8"), Got::Ok]);
        let judged = gate(&agent(), &[a], false);
        assert_eq!(judged.kind, Kind::Pass);
        assert_eq!(judged.carriers, ['a']);
        assert!(
            judged
                .text
                .contains("desktop: no control could create one ((a) 8), information")
        );
        assert!(
            judged
                .text
                .contains("clipboard: discriminating, carried by (a)")
        );
    }

    #[test]
    fn a_desktop_only_control_with_the_agent_refused_passes() {
        let clip = no("failed OpenClipboard 5");
        let b = control('b', Rank::Close, [Got::Ok, no("1"), clip]);
        let judged = gate(&agent(), &[b], false);
        assert_eq!(judged.kind, Kind::Pass);
        assert_eq!(judged.carriers, ['b']);
    }

    #[test]
    fn a_working_control_with_the_agent_succeeding_fails() {
        let a = control('a', Rank::Close, [Got::Ok, no("1"), Got::Ok]);
        let mut open = agent();
        open[2] = Got::Ok;
        assert_eq!(
            gate(&open, std::slice::from_ref(&a), false).kind,
            Kind::Fail
        );
        let mut odd = agent();
        odd[0] = no("8");
        assert_eq!(gate(&odd, &[a], false).kind, Kind::Fail);
    }

    #[test]
    fn no_working_control_is_info() {
        let a = control(
            'a',
            Rank::Close,
            [no("8"), no("1"), no("failed OpenClipboard 5")],
        );
        let e = control('e', Rank::Far, [Got::Ok, no("1"), Got::Ok]);
        let judged = gate(&agent(), &[a, e], false);
        assert_eq!(judged.kind, Kind::Info);
        assert!(judged.text.contains("(e) also did, evidence only"));
        assert_eq!(gate(&agent(), &[], false).kind, Kind::Info);
    }

    #[test]
    fn a_variant_whose_shape_is_wrong_carries_nothing() {
        let mut a = control('a', Rank::Close, [Got::Ok, Got::Ok, Got::Ok]);
        a.shape = Err("probe ui-control ran on station Some(\"WinSta0\")".to_owned());
        assert_eq!(gate(&agent(), &[a], false).kind, Kind::Info);
    }

    #[test]
    fn strict_mode_needs_each_effect_discriminating_and_refused() {
        let all = control('a', Rank::Close, [Got::Ok, Got::Ok, Got::Ok]);
        assert_eq!(gate(&agent(), &[all], true).kind, Kind::Pass);
        let no_switch = control('a', Rank::Close, [Got::Ok, no("1"), Got::Ok]);
        assert_eq!(
            gate(&agent(), std::slice::from_ref(&no_switch), true).kind,
            Kind::Fail
        );
        assert_eq!(gate(&agent(), &[no_switch], false).kind, Kind::Pass);
        assert_eq!(gate(&agent(), &[], true).kind, Kind::Fail);
    }

    #[test]
    fn when_g_runs_only_g_carries_an_effect() {
        let g = control('g', Rank::Agent, [no("8"), no("1"), Got::Ok]);
        let a = control('a', Rank::Close, [Got::Ok, no("1"), Got::Ok]);
        let judged = gate(&agent(), &[g.clone(), a.clone()], false);
        assert_eq!(judged.kind, Kind::Pass);
        assert_eq!(judged.carriers, ['g']);
        assert!(judged.text.contains("desktop: no control could create one"));
        let idle = control(
            'g',
            Rank::Agent,
            [no("8"), no("1"), no("failed OpenClipboard 0")],
        );
        assert_eq!(gate(&agent(), &[idle, a.clone()], false).kind, Kind::Info);
        let mut wrong = g;
        wrong.shape = Err("station".to_owned());
        assert_eq!(gate(&agent(), &[wrong, a], false).carriers, ['a']);
    }

    #[test]
    fn the_lines_read_into_results() {
        let lines: Vec<String> = [
            "ui-effect desktop-create 5",
            "ui-effect desktop-create-default-heap 0",
            "ui-effect switch-desktop create-failed 5",
            "ui-effect clipboard-write failed OpenClipboard 0x80070005",
        ]
        .map(str::to_owned)
        .to_vec();
        let got = read(&lines, "ui-effect");
        assert_eq!(
            got,
            [
                refused("5"),
                refused("create-failed 5"),
                refused("failed OpenClipboard 0x80070005")
            ]
        );
        let other = |line: &str| read(&[line.to_owned()], "ui-control");
        assert_eq!(other("ui-control desktop-create 0")[0], Got::Ok);
        assert_eq!(other("ui-control switch-desktop 1")[1], no("1"));
        assert_eq!(other("ui-control clipboard-write ok")[2], Got::Ok);
        let window = other("ui-control clipboard-write failed CreateWindowExW 5");
        assert_eq!(window[2], no("failed CreateWindowExW 5"));
        assert_eq!(
            other("ui-control clipboard-write failed SetClipboardData 1418")[2],
            no("failed SetClipboardData 1418")
        );
        assert_eq!(other("nothing")[0], no("none printed"));
    }
}
