//! The positive control for the UI-limit effect ([`controls`]): when the agent's innermost job
//! reads `0x0` and its `CreateDesktopW` and clipboard write both gave 5, the harness runs
//! `probe ui-control` once per [`VARIANTS`] entry, each on a fresh `ClauseoS15Ctl-<hex>` station
//! and outside any job of ours, and prints each result. The control is the first variant, closest
//! to the agent first, whose desktop create and clipboard write both succeed ([`choose`]); only
//! one at (d) or closer ([`GATE`]) lets the row pass.

use std::time::Duration;

use clauseo_sandbox::windows::secrets::secret_for;
use clauseo_sandbox::windows::sids::Sid;

use super::checks_ui::random;
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
    /// `a` to `f`, closest to the agent first.
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

/// The first index that can't pass alone: (e) and (f) succeeding leave the row INFO.
const GATE: usize = 4;

/// The positive control: every variant run and noted, then the first that succeeded chosen. A
/// pass names the variant and how it differs from the agent; (e) or (f) first, or none, calls
/// [`Ctx::info`] with every variant's codes, so the row is INFO, never PASS.
pub(crate) fn controls(ctx: &Ctx<'_>) -> String {
    let lawyer = match win::own_token() {
        Ok(facts) => facts.user,
        Err(e) => {
            let why = format!("the harness's token can't be read ({e})");
            ctx.info(&format!("the positive control can't run: {why}"));
            return format!("the control didn't run ({why})");
        }
    };
    ctx.note(&format!(
        "positive control: probe ui-control once per variant (a) to (f), each on a fresh \
         ClauseoS15Ctl-<hex> station made as the outer runner makes ClauseoRun-… (GENERIC_ALL; \
         DACL SYSTEM and the token's user, the harness's user too; Low label), outside any job \
         of ours; the first whose desktop create and clipboard write both succeed is the \
         control, and only (a) to (d) can pass; {}",
        win_ctl::own_job()
    ));
    let mut outcomes = Vec::new();
    for variant in &VARIANTS {
        let pool = ctx.manifest.pools.first().map(|p| p.sid.as_str());
        let user = match variant.source {
            Source::Pool => pool.unwrap_or("(no pool user)"),
            Source::Lawyer(_) => lawyer.as_str(),
        };
        let outcome = run_variant(ctx, variant, &lawyer).and_then(|lines| {
            let seen = facts(&lines);
            ctx.note(&format!("control ({}): {seen}", variant.key));
            verdict(variant, user, &lines).map_err(|why| format!("{why} ({seen})"))
        });
        let said = outcome
            .as_ref()
            .map_or_else(Clone::clone, |()| "succeeded".to_owned());
        ctx.note(&format!("control ({}): {said}", variant.key));
        outcomes.push(outcome);
    }
    let evidence = VARIANTS
        .iter()
        .zip(&outcomes)
        .map(|(v, o)| match o {
            Ok(()) => format!("({}) succeeded", v.key),
            Err(why) => format!("({}) {why}", v.key),
        })
        .collect::<Vec<_>>()
        .join("; ");
    match choose(&outcomes).and_then(|i| VARIANTS.get(i).map(|v| (i, v))) {
        Some((i, v)) if i < GATE => format!(
            "the control, variant ({}), made a desktop and wrote the clipboard outside the job; \
             it differs from the agent {}",
            v.key, v.differs
        ),
        Some((_, v)) => {
            let plain = if v.key == 'f' {
                "; the control is unrestricted, the lawyer's own token, and the agent's 5s stand \
                 against its success"
            } else {
                ""
            };
            ctx.info(&format!(
                "only variant ({}) made a desktop and wrote the clipboard; it differs from the \
                 agent {}{plain}; only (d) or closer counts, so this is evidence for a decision: \
                 {evidence}",
                v.key, v.differs
            ));
            format!("the control is ({}) alone, evidence only", v.key)
        }
        None => {
            ctx.info(&format!(
                "no control variant made a desktop and wrote the clipboard, so the agent's 5s \
                 prove nothing: {evidence}"
            ));
            "no control variant succeeded".to_owned()
        }
    }
}

/// The index of the first variant that succeeded.
fn choose(outcomes: &[Result<(), String>]) -> Option<usize> {
    outcomes.iter().position(Result::is_ok)
}

/// Makes `variant`'s station and token and runs `probe ui-control` there; its lines. The call
/// that failed and its code otherwise.
fn run_variant(ctx: &Ctx<'_>, variant: &Variant, lawyer: &str) -> Result<Vec<String>, String> {
    let station = format!("ClauseoS15Ctl-{}", random()?);
    let desktop = format!("{station}\\Default");
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
            let args = ["probe", "ui-relay", desktop.as_str(), lawyer, ms.as_str()];
            let ran =
                win_logon::run_with_logon(&pool.name, password, &desktop, &exe, &args, &bin, limit);
            drop(held);
            ran?
        }
        Source::Lawyer(shape) => {
            let (token, _) = win_ctl::shaped_token(shape, None)?;
            let held = open(&[lawyer])?;
            let args: &[&str] = if variant.sa {
                &["probe", "ui-control", "sa"]
            } else {
                &["probe", "ui-control"]
            };
            let ran = win_ctl::run_as(&token, &desktop, &exe, args, &bin, limit);
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

/// A variant's rule, on its `probe ui-control` lines: it ran on a `ClauseoS15Ctl-` station as
/// `user`, with the integrity and restricting SIDs `variant` names (and explicit attributes for
/// (c)), `desktop-create 0` and `clipboard-write ok`. Otherwise why, naming the call and code.
fn verdict(variant: &Variant, user: &str, lines: &[String]) -> Result<(), String> {
    let station = get(lines, "station");
    if !station.is_some_and(|s| s.starts_with("ClauseoS15Ctl-")) {
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
    match get(lines, "desktop-create") {
        Some("0") => {}
        other => return Err(format!("its CreateDesktopW gave {other:?}, not 0")),
    }
    match get(lines, "clipboard-write") {
        Some("ok") => Ok(()),
        other => Err(format!("its clipboard write gave {other:?}, not ok")),
    }
}

#[cfg(test)]
mod tests {
    use super::{GATE, VARIANTS, choose, verdict};

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|l| (*l).to_owned()).collect()
    }

    #[test]
    fn the_first_success_is_the_control_and_only_d_or_closer_passes() {
        let keys: String = VARIANTS.iter().map(|v| v.key).collect();
        assert_eq!(keys, "abcdef");
        assert_eq!(VARIANTS.get(GATE).map(|v| v.key), Some('e'));
        let no = || Err::<(), String>("8".to_owned());
        assert_eq!(choose(&[no(), Ok(()), Ok(())]), Some(1));
        assert_eq!(choose(&[no(), no()]), None);
    }

    #[test]
    fn a_variant_counts_only_when_both_steps_work_under_its_own_shape() {
        let good = [
            "ui-control station ClauseoS15Ctl-00",
            "ui-control user S-1-5-21-1-1001",
            "ui-control integrity S-1-16-4096",
            "ui-control restricted 4",
            "ui-control desktop-sa default",
            "ui-control desktop-create 0",
            "ui-control clipboard-write ok",
        ];
        let [pool, lawyer, sa, not_low, unrestricted, plain] = &VARIANTS;
        let user = "S-1-5-21-1-1001";
        assert_eq!(verdict(pool, user, &lines(&good)), Ok(()));
        assert_eq!(verdict(lawyer, user, &lines(&good)), Ok(()));
        assert!(verdict(pool, "S-1-5-21-1-1002", &lines(&good)).is_err());
        assert!(verdict(sa, user, &lines(&good)).is_err());
        assert!(verdict(not_low, user, &lines(&good)).is_err());
        assert!(verdict(unrestricted, user, &lines(&good)).is_err());
        assert!(verdict(plain, user, &lines(&good)).is_err());
        let swap = |at: usize, line: &str| {
            let mut changed = good;
            if let Some(slot) = changed.get_mut(at) {
                *slot = line;
            }
            lines(&changed)
        };
        assert_eq!(
            verdict(sa, user, &swap(4, "ui-control desktop-sa explicit")),
            Ok(())
        );
        let medium = swap(2, "ui-control integrity S-1-16-8192");
        assert_eq!(verdict(not_low, user, &medium), Ok(()));
        assert!(verdict(lawyer, user, &medium).is_err());
        assert_eq!(
            verdict(unrestricted, user, &swap(3, "ui-control restricted 0")),
            Ok(())
        );
        assert!(verdict(lawyer, user, &swap(0, "ui-control station WinSta0")).is_err());
        assert!(verdict(lawyer, user, &swap(5, "ui-control desktop-create 8")).is_err());
        let clip = swap(6, "ui-control clipboard-write failed OpenClipboard 5");
        assert!(verdict(lawyer, user, &clip).is_err());
        assert!(verdict(lawyer, user, &lines(&good[..6])).is_err());
    }
}
