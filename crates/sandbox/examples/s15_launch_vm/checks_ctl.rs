//! The positive control for the UI-limit effects ([`controls`]): when the agent's innermost job
//! reads `0x0` (or in strict mode), the harness runs `probe ui-control` once per [`VARIANTS`]
//! entry, each on a fresh `ClauseoS15Ctl-<hex>` station and outside any job of ours, and, while
//! the agent still runs, variant [`G`] on the agent's own `ClauseoRun-<hex>\Default`. Each
//! result is printed; `gate.rs` decides per effect which variants carry it.

use std::time::Duration;

use clauseo_sandbox::windows::secrets::secret_for;
use clauseo_sandbox::windows::sids::Sid;

use super::checks_ui::random;
use super::gate::{self, Control, Got, Judged, Rank};
use super::rows::Ctx;
use super::win_ctl::{self, Ran, Shape};
use super::{win, win_desk, win_logon};

/// How long one variant's process may run (or the row's limit, when shorter).
const CONTROL_LIMIT: Duration = Duration::from_secs(15);
/// How much sooner than the harness the relay of variant (a) ends its own child.
const RELAY_MARGIN: Duration = Duration::from_secs(5);

/// Whose token a variant's process runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// Matter A's pool user: the harness starts `probe ui-relay` as that user with
    /// `CreateProcessWithLogonW` (as the outer runner starts the inner), and the relay makes the
    /// agent's token from its own (as the inner runner does) and starts `probe ui-control`.
    Pool,
    /// The harness's own (the lawyer's) token, made into `Shape` ([`win_ctl::shaped_token`]).
    Lawyer(Shape),
}

/// One control variant.
#[derive(Debug)]
struct Variant {
    /// `a` to `g`.
    key: char,
    source: Source,
    /// `probe ui-control sa`: `CreateDesktopW` gets explicit `SECURITY_ATTRIBUTES`.
    sa: bool,
    /// How it differs from the agent, for the row's note.
    differs: &'static str,
    /// The restricting SIDs its token must hold.
    restricted: usize,
    /// Whether its token must be Low (else it must not be).
    low: bool,
}

/// The variants, closest to the agent first.
const VARIANTS: [Variant; 6] = [
    Variant {
        key: 'a',
        source: Source::Pool,
        sa: false,
        differs: "only in not being in the runner's job: the pool user's own token, made into \
                  the agent's by the inner runner's recipe (restricting the pool user, S-1-5-12, \
                  S-1-5-32-545, S-1-1-0; the logon SID deny-only; Low; DISABLE_MAX_PRIVILEGE; \
                  the agent's default DACL), started through CreateProcessWithLogonW and a relay \
                  as the outer and inner runners start the agent; besides, its station's DACL \
                  adds the harness's user and the token object's own DACL isn't set",
        restricted: 4,
        low: true,
    },
    Variant {
        key: 'b',
        source: Source::Lawyer(Shape::Agent),
        sa: false,
        differs: "in its user (the lawyer, not the pool user: restricting the lawyer, S-1-5-12, \
                  S-1-5-32-545, S-1-1-0; default DACL the lawyer and SYSTEM) and in not being in \
                  the job; Low, restricted, the logon SID deny-only, as the agent",
        restricted: 4,
        low: true,
    },
    Variant {
        key: 'c',
        source: Source::Lawyer(Shape::Agent),
        sa: true,
        differs: "as (b), and its CreateDesktopW gets explicit SECURITY_ATTRIBUTES (a DACL of \
                  the token's user and SYSTEM)",
        restricted: 4,
        low: true,
    },
    Variant {
        key: 'd',
        source: Source::Lawyer(Shape::NotLow),
        sa: false,
        differs: "as (b), and it stays at the lawyer's own integrity level, not Low",
        restricted: 4,
        low: false,
    },
    Variant {
        key: 'e',
        source: Source::Lawyer(Shape::Unrestricted),
        sa: false,
        differs: "as (b), and it has no restricting SIDs: the control is Low but unrestricted",
        restricted: 0,
        low: true,
    },
    Variant {
        key: 'f',
        source: Source::Lawyer(Shape::Plain),
        sa: false,
        differs: "in everything but the station kind: the control is the lawyer's own token, \
                  unrestricted, not Low, its privileges kept, run as a child process",
        restricted: 0,
        low: false,
    },
];

/// The first index that can't carry an effect: (e) and (f) are evidence only.
const GATE: usize = 4;

/// The closest variant: the agent's token shape on the agent's own station, outside the job.
const G: Variant = Variant {
    key: 'g',
    source: Source::Pool,
    sa: false,
    differs: "only in not being in the runner's job: the agent's token shape (pool user A, Low, \
              the four restricting SIDs, the logon SID deny-only) made by a relay running as \
              the pool user, as for (a), and started on the agent's own ClauseoRun-<hex>\\Default \
              while the agent waits",
    restricted: 4,
    low: true,
};

/// Strict mode's `SwitchDesktop` control: variant (f)'s token on the user's own `WinSta0\Default`.
const W: Variant = Variant {
    key: 'w',
    source: Source::Lawyer(Shape::Plain),
    sa: false,
    differs: "in its station as well as the job: variant (f)'s token, the lawyer's own, on the \
              interactive WinSta0\\Default",
    restricted: 0,
    low: false,
};

/// Whether variant (g) can run: the agent still runs (and its station with it), or why not.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Live<'a> {
    Yes,
    No(&'a str),
}

/// Which station a variant's `probe ui-control` must report.
#[derive(Debug, Clone, Copy)]
enum Station<'a> {
    /// A fresh `ClauseoS15Ctl-<hex>`.
    Control,
    /// The agent's own, by the name it printed (`ClauseoRun-<hex>`).
    Agent(&'a str),
    /// The user's interactive `WinSta0` (strict mode's (w)).
    Interactive,
}

/// The positive control: (g) when the agent is `live`, then (a) to (f), each run and noted,
/// and [`gate::gate`] on the agent's `ui-effect` lines and theirs. The text names the carriers,
/// how each differs from the agent, and why (g) didn't run when it didn't.
pub(crate) fn controls(
    ctx: &Ctx<'_>,
    agent_lines: &[String],
    live: Live<'_>,
    strict: bool,
) -> Judged {
    let agent = gate::read(agent_lines, "ui-effect");
    let lawyer = match win::own_token() {
        Ok(facts) => facts.user,
        Err(e) => {
            let mut judged = gate::gate(&agent, &[], strict);
            judged.text = format!(
                "no control ran: the harness's token can't be read ({e}); {}",
                judged.text
            );
            return judged;
        }
    };
    ctx.note(&format!(
        "positive control: probe ui-control as variant (g) on the agent's own station while it \
         waits, then (a) to (f), each on a fresh ClauseoS15Ctl-<hex> station made as the outer \
         runner makes ClauseoRun-… (GENERIC_ALL; DACL SYSTEM and the token's user, the \
         harness's user too; Low label); none in a job of ours. Per effect, (g) alone carries it \
         when (g) ran, else (a) to (d); {}",
        win_ctl::own_job()
    ));
    let mut runs = Vec::new();
    let g_note = match (live, agent_station(agent_lines)) {
        (Live::No(why), _) => format!("(g) didn't run: {why}"),
        (Live::Yes, Err(why)) => format!("(g) didn't run: {why}"),
        (Live::Yes, Ok(station)) => match one(ctx, &G, &lawyer, Station::Agent(station)) {
            Ok(run) => {
                let said = match &run.shape {
                    Ok(()) => format!("(g) ran on the agent's own station {station}"),
                    Err(why) => format!("(g) ran but doesn't count: {why}"),
                };
                runs.push(run);
                said
            }
            Err(why) => format!("(g) didn't run: {why}"),
        },
    };
    ctx.note(&format!("control (g): {g_note}"));
    for (at, variant) in VARIANTS.iter().enumerate() {
        let rank = if at < GATE { Rank::Close } else { Rank::Far };
        let run = one(ctx, variant, &lawyer, Station::Control).unwrap_or_else(|why| Control {
            key: variant.key,
            rank,
            shape: Err(why),
            got: gate::EFFECTS.map(|_| Got::Failed("didn't run".to_owned())),
        });
        let said = run
            .shape
            .as_ref()
            .map_or_else(Clone::clone, |()| "ran".to_owned());
        ctx.note(&format!("control ({}): {said}", variant.key));
        runs.push(run);
    }
    if strict {
        let run = one(ctx, &W, &lawyer, Station::Interactive).unwrap_or_else(|why| Control {
            key: W.key,
            rank: Rank::Interactive,
            shape: Err(why),
            got: gate::EFFECTS.map(|_| Got::Failed("didn't run".to_owned())),
        });
        let said = run
            .shape
            .as_ref()
            .map_or_else(Clone::clone, |()| "ran on WinSta0".to_owned());
        ctx.note(&format!("control (w): {said}"));
        runs.push(run);
    }
    let mut judged = gate::gate(&agent, &runs, strict);
    let differs = judged
        .carriers
        .iter()
        .filter_map(|key| {
            [&G, &W]
                .into_iter()
                .chain(&VARIANTS)
                .find(|v| v.key == *key)
        })
        .map(|v| format!("({}) differs from the agent {}", v.key, v.differs))
        .collect::<Vec<_>>();
    let differs = if differs.is_empty() {
        String::new()
    } else {
        format!("; {}", differs.join("; "))
    };
    judged.text = format!("{}; {g_note}{differs}", judged.text);
    judged
}

/// The agent's own station, from its `ui-effect station` line: a `ClauseoRun-` name.
fn agent_station(lines: &[String]) -> Result<&str, String> {
    let station = lines
        .iter()
        .find_map(|l| l.strip_prefix("ui-effect station "))
        .map(str::trim);
    match station {
        Some(name) if name.starts_with("ClauseoRun-") => Ok(name),
        other => Err(format!(
            "the agent printed station {other:?}, not a ClauseoRun- one"
        )),
    }
}

/// Runs `variant` (its `probe ui-control` on `on`) and reads it: its shape check and its
/// three results. The call that failed and its code when it didn't run.
fn one(ctx: &Ctx<'_>, variant: &Variant, lawyer: &str, on: Station<'_>) -> Result<Control, String> {
    let pool = ctx.manifest.pools.first().map(|p| p.sid.as_str());
    let user = match variant.source {
        Source::Pool => pool.unwrap_or("(no pool user)"),
        Source::Lawyer(_) => lawyer,
    };
    let lines = run_variant(ctx, variant, lawyer, on)?;
    let seen = facts(&lines);
    ctx.note(&format!("control ({}): {seen}", variant.key));
    let rank = match on {
        Station::Agent(_) => Rank::Agent,
        Station::Interactive => Rank::Interactive,
        Station::Control if VARIANTS.iter().position(|v| v.key == variant.key) < Some(GATE) => {
            Rank::Close
        }
        Station::Control => Rank::Far,
    };
    Ok(Control {
        key: variant.key,
        rank,
        shape: shape(variant, user, &lines, on).map_err(|why| format!("{why} ({seen})")),
        got: gate::read(&lines, "ui-control"),
    })
}

/// Makes `variant`'s station and token and runs `probe ui-control` there (for (g), the relay on a
/// fresh control station and `probe ui-control` on the agent's `on`); its lines. The call that
/// failed and its code otherwise.
fn run_variant(
    ctx: &Ctx<'_>,
    variant: &Variant,
    lawyer: &str,
    on: Station<'_>,
) -> Result<Vec<String>, String> {
    let station = format!("ClauseoS15Ctl-{}", random()?);
    let desktop = format!("{station}\\Default");
    let target = match on {
        Station::Control => desktop.clone(),
        Station::Agent(name) => format!("{name}\\Default"),
        Station::Interactive => "WinSta0\\Default".to_owned(),
    };
    let limit = ctx.limit.min(CONTROL_LIMIT);
    let exe = ctx.manifest.harness_exe();
    let bin = ctx.manifest.bin();
    let open = |sids: &[&str]| {
        win_desk::ControlStation::open(&station, sids)
            .map_err(|(call, code)| format!("{call} gave {code} (station {station})"))
    };
    let ran: Ran = match variant.source {
        Source::Pool => {
            let pool = ctx
                .manifest
                .pools
                .first()
                .ok_or("the manifest names no pool user")?;
            let sid = Sid::parse(&pool.sid).map_err(|e| e.to_string())?;
            let held = open(&[lawyer, pool.sid.as_str()])?;
            // The crate's error text never holds the password.
            let password = secret_for(&ctx.manifest.data_dir, &sid)
                .map_err(|e| format!("secret_for({}): {e}", pool.sid))?;
            let ms = limit.saturating_sub(RELAY_MARGIN).as_millis().to_string();
            let args = ["probe", "ui-relay", target.as_str(), lawyer, ms.as_str()];
            let ran =
                win_logon::run_with_logon(&pool.name, password, &desktop, &exe, &args, &bin, limit);
            drop(held);
            ran?
        }
        Source::Lawyer(shape) => {
            let (token, _) = win_ctl::shaped_token(shape, None)?;
            // (w) runs on the user's own WinSta0, which needs no station of ours.
            let held = match on {
                Station::Interactive => None,
                _ => Some(open(&[lawyer])?),
            };
            let args: &[&str] = if variant.sa {
                &["probe", "ui-control", "sa"]
            } else {
                &["probe", "ui-control"]
            };
            let ran = win_ctl::run_as(&token, &target, &exe, args, &bin, limit);
            drop(held);
            ran?
        }
    };
    ctx.note(&format!(
        "control ({}): pid {}, exit {:?} (None: ended at the limit)",
        variant.key, ran.pid, ran.code
    ));
    for line in &ran.lines {
        ctx.note(&format!("control ({}): {line}", variant.key));
    }
    if ran.code.is_none() {
        return Err("its process overran its limit and was ended".to_owned());
    }
    if let Some(why) = ran
        .lines
        .iter()
        .find_map(|l| l.strip_prefix("ui-relay error "))
    {
        return Err(format!("the relay failed: {why}"));
    }
    Ok(ran.lines)
}

/// The value of `probe ui-control`'s `key` line in `lines`.
fn get<'l>(lines: &'l [String], key: &str) -> Option<&'l str> {
    lines
        .iter()
        .find_map(|l| l.strip_prefix(&format!("ui-control {key} ")))
        .map(str::trim)
}

/// The codes and token facts a variant printed, as one note.
fn facts(lines: &[String]) -> String {
    let keys = [
        "desktop-create",
        "desktop-create-default-heap",
        "switch-desktop",
        "switch-desktop-back",
        "clipboard-write",
        "desktop-sa",
        "user",
        "integrity",
        "restricted",
        "ui-limits",
        "station",
    ];
    keys.iter()
        .map(|key| format!("{key} {}", get(lines, key).unwrap_or("none")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A variant's shape, on its `probe ui-control` lines: it ran on the station `on` names (a
/// `ClauseoS15Ctl-` one, or exactly the agent's `ClauseoRun-` one) as `user`, with the integrity
/// and restricting SIDs `variant` names (and explicit attributes for (c)). Otherwise why. What it
/// got at each effect is `gate.rs`'s.
fn shape(variant: &Variant, user: &str, lines: &[String], on: Station<'_>) -> Result<(), String> {
    let station = get(lines, "station");
    let right = match on {
        Station::Control => station.is_some_and(|s| s.starts_with("ClauseoS15Ctl-")),
        Station::Agent(name) => station == Some(name) && name.starts_with("ClauseoRun-"),
        Station::Interactive => station == Some("WinSta0"),
    };
    if !right {
        return Err(format!("probe ui-control ran on station {station:?}"));
    }
    let integrity = get(lines, "integrity");
    let low = integrity == Some(win_ctl::LOW);
    let restricted = variant.restricted.to_string();
    if get(lines, "user") != Some(user)
        || integrity.is_none()
        || low != variant.low
        || get(lines, "restricted") != Some(restricted.as_str())
    {
        return Err(format!(
            "its token isn't the variant's: user {:?}, integrity {integrity:?}, restricting \
             SIDs {:?}",
            get(lines, "user"),
            get(lines, "restricted")
        ));
    }
    let sa = if variant.sa { "explicit" } else { "default" };
    if get(lines, "desktop-sa") != Some(sa) {
        return Err(format!(
            "its desktop attributes were {:?}, not {sa}",
            get(lines, "desktop-sa")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{G, GATE, Station, VARIANTS, W, agent_station, shape};

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|l| (*l).to_owned()).collect()
    }

    const GOOD: [&str; 5] = [
        "ui-control station ClauseoS15Ctl-00",
        "ui-control user S-1-5-21-1-1001",
        "ui-control integrity S-1-16-4096",
        "ui-control restricted 4",
        "ui-control desktop-sa default",
    ];

    fn swap(at: usize, line: &'static str) -> Vec<String> {
        let mut changed = GOOD;
        if let Some(slot) = changed.get_mut(at) {
            *slot = line;
        }
        lines(&changed)
    }

    #[test]
    fn the_variants_run_closest_first_and_only_d_or_closer_carry() {
        let keys: String = VARIANTS.iter().map(|v| v.key).collect();
        assert_eq!(keys, "abcdef");
        assert_eq!(VARIANTS.get(GATE).map(|v| v.key), Some('e'));
        assert_eq!((G.key, G.restricted, G.low), ('g', 4, true));
    }

    #[test]
    fn w_counts_only_on_winsta0_as_the_lawyer_unrestricted() {
        let user = "S-1-5-21-1-1001";
        let on_winsta0 = lines(&[
            "ui-control station WinSta0",
            "ui-control user S-1-5-21-1-1001",
            "ui-control integrity S-1-16-12288",
            "ui-control restricted 0",
            "ui-control desktop-sa default",
        ]);
        assert!(shape(&W, user, &on_winsta0, Station::Interactive).is_ok());
        let mut fresh = on_winsta0.clone();
        if let Some(line) = fresh.first_mut() {
            *line = "ui-control station ClauseoS15Ctl-00".to_owned();
        }
        assert!(shape(&W, user, &fresh, Station::Interactive).is_err());
        assert!(shape(&W, "S-1-5-21-1-1002", &on_winsta0, Station::Interactive).is_err());
    }

    #[test]
    fn a_mismatched_station_user_or_integrity_disqualifies_a_variant() {
        let [pool, lawyer, sa, not_low, unrestricted, plain] = &VARIANTS;
        let user = "S-1-5-21-1-1001";
        let on = Station::Control;
        assert_eq!(shape(pool, user, &lines(&GOOD), on), Ok(()));
        assert_eq!(shape(lawyer, user, &lines(&GOOD), on), Ok(()));
        assert!(shape(pool, "S-1-5-21-1-1002", &lines(&GOOD), on).is_err());
        assert!(shape(sa, user, &lines(&GOOD), on).is_err());
        assert!(shape(not_low, user, &lines(&GOOD), on).is_err());
        assert!(shape(unrestricted, user, &lines(&GOOD), on).is_err());
        assert!(shape(plain, user, &lines(&GOOD), on).is_err());
        let explicit = swap(4, "ui-control desktop-sa explicit");
        assert_eq!(shape(sa, user, &explicit, on), Ok(()));
        let medium = swap(2, "ui-control integrity S-1-16-8192");
        assert_eq!(shape(not_low, user, &medium, on), Ok(()));
        assert!(shape(lawyer, user, &medium, on).is_err());
        let open = swap(3, "ui-control restricted 0");
        assert_eq!(shape(unrestricted, user, &open, on), Ok(()));
        assert!(shape(lawyer, user, &swap(0, "ui-control station WinSta0"), on).is_err());
        let agents = swap(0, "ui-control station ClauseoRun-0123456789abcdef");
        assert!(shape(pool, user, &agents, on).is_err());
        assert!(shape(lawyer, user, &lines(&GOOD[..4]), on).is_err());
    }

    #[test]
    fn variant_g_must_be_on_the_agents_own_station_as_pool_user_low_restricted() {
        let user = "S-1-5-21-1-1001";
        let name = "ClauseoRun-0123456789abcdef";
        let on = Station::Agent(name);
        let agents = swap(0, "ui-control station ClauseoRun-0123456789abcdef");
        assert_eq!(shape(&G, user, &agents, on), Ok(()));
        assert!(shape(&G, user, &lines(&GOOD), on).is_err());
        let other = swap(0, "ui-control station ClauseoRun-fedcba9876543210");
        assert!(shape(&G, user, &other, on).is_err());
        let ctl = Station::Agent("ClauseoS15Ctl-00");
        assert!(shape(&G, user, &lines(&GOOD), ctl).is_err());
        assert!(shape(&G, "S-1-5-21-1-500", &agents, on).is_err());
        let mut medium = agents.clone();
        medium[2] = "ui-control integrity S-1-16-8192".to_owned();
        assert!(shape(&G, user, &medium, on).is_err());
        let mut open = agents;
        open[3] = "ui-control restricted 0".to_owned();
        assert!(shape(&G, user, &open, on).is_err());
        let printed = lines(&["ui-effect station ClauseoRun-0123456789abcdef"]);
        assert_eq!(agent_station(&printed), Ok(name));
        assert!(agent_station(&lines(&["ui-effect station ClauseoS15Ctl-00"])).is_err());
        assert!(agent_station(&[]).is_err());
    }
}
