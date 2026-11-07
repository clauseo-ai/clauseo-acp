//! The verdicts of the pool rows (`checks_pool.rs`) and of `breakaway` (`checks_sec.rs`), from
//! the lines the harness and the probes print: no Win32 call here, so each rule has a unit test.
//!
//! The harness's own lines:
//!
//! - `pool <pid> in-job=<true|false|error:<code>|unchecked> <image>`: one per process whose
//!   token user is the session's pool user (`win_pool::pool_processes`); `in-job` is
//!   `IsProcessInJob` against the session's kill-on-close job. Then `pool-unreadable <pid>
//!   <image>` per process whose user the harness couldn't read, and `pool-unknown <n>`, their
//!   count; or `pool-error <why>` alone. An unreadable process may be the pool user's, so a
//!   listing with one never passes.
//! - `benign ran <pid>` or `benign never-ran <why>`: the benign `probe wait` the harness started
//!   as the pool user outside the job printed its pid (it ran) or didn't; then `benign gone <ms>
//!   exit <code>` (it had ended `ms` after it was started) or `benign alive <ms>`.
//! - `breakaway our-job <true|false|error <why>>`: whether the breakaway child is in the
//!   session's kill-on-close job.

use std::collections::BTreeSet;

use super::checks::{DENIED, DENY_ONLY, LOGON_ID, values};

/// A rule's verdict. `Info`: the row's own positive control failed, so it proves nothing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// It held; a note on what was seen.
    Pass(String),
    /// It didn't; why.
    Fail(String),
    /// Nothing could be checked; why.
    Info(String),
}

/// How long `sweep-mid-session` gives the runner's sweep (it runs every 30 s).
pub(crate) const MID_SESSION_MS: u128 = 45_000;

/// One `pool` line.
#[derive(Debug)]
struct Pooled<'l> {
    pid: u32,
    in_job: &'l str,
    image: &'l str,
}

/// The `pool` lines and the `pool-unknown` count; the `pool-error` text when there is one.
fn pooled(lines: &[String]) -> Result<(Vec<Pooled<'_>>, usize), String> {
    if values(lines, "pool-unknown").is_empty() && values(lines, "pool-error").is_empty() {
        return Err(format!(
            "the harness's listing has no pool-unknown line: {lines:?}"
        ));
    }
    if let Some(why) = values(lines, "pool-error").first() {
        return Err(format!("the harness's listing failed: {why}"));
    }
    let found = values(lines, "pool")
        .into_iter()
        .filter_map(|rest| {
            let mut words = rest.splitn(3, ' ');
            let pid = words.next()?.parse().ok()?;
            let in_job = words.next()?.strip_prefix("in-job=")?;
            Some(Pooled {
                pid,
                in_job,
                image: words.next().unwrap_or("?"),
            })
        })
        .collect();
    let unknown = values(lines, "pool-unknown")
        .first()
        .and_then(|n| n.parse().ok())
        .unwrap_or(usize::MAX);
    Ok((found, unknown))
}

/// INFO when `unknown` processes' users couldn't be read: the count and their images.
fn unreadable(lines: &[String], unknown: usize, what: &str) -> Option<Verdict> {
    if unknown == 0 {
        return None;
    }
    let images: Vec<&str> = values(lines, "pool-unreadable")
        .into_iter()
        .map(|rest| rest.split_once(' ').map_or("?", |(_, image)| image))
        .collect();
    Some(Verdict::Info(format!(
        "{what}, but the harness couldn't read the user of {unknown} processes ({}), any of which \
         may be the pool user's, so it proves nothing",
        images.join(", ")
    )))
}

fn named(found: &[&Pooled<'_>]) -> String {
    let each: Vec<String> = found
        .iter()
        .map(|p| format!("{} ({}, in-job={})", p.pid, p.image, p.in_job))
        .collect();
    each.join(", ")
}

/// `pool-processes-at-start`: the listing taken once the agent (`agent`, its pid) printed its
/// first line. PASS when every process's user was read and every process of the pool user is
/// in the session's kill-on-close job; FAIL naming each that isn't (or couldn't be checked);
/// INFO when the listing didn't see the agent itself, or couldn't read some process's user.
pub(crate) fn at_start(lines: &[String], agent: u32) -> Verdict {
    let (found, unknown) = match pooled(lines) {
        Ok(read) => read,
        Err(why) => return Verdict::Fail(why),
    };
    if !found.iter().any(|p| p.pid == agent) {
        return Verdict::Info(format!(
            "the harness's listing didn't see the agent ({agent}) as the pool user ({unknown} \
             processes' users unknown), so it proves nothing"
        ));
    }
    let outside: Vec<&Pooled<'_>> = found.iter().filter(|p| p.in_job != "true").collect();
    if !outside.is_empty() {
        return Verdict::Fail(format!(
            "the pool user's processes outside the session's kill-on-close job: {}",
            named(&outside)
        ));
    }
    let note = format!(
        "{} processes of the pool user, the agent ({agent}) among them, all in the session's \
         kill-on-close job",
        found.len()
    );
    unreadable(lines, unknown, &note).unwrap_or(Verdict::Pass(note))
}

/// `pool-processes-at-end`: `running`, the listing while the session ran (it must see the agent,
/// as in [`at_start`], else INFO); `after`, the listing `waited_ms` after the session ended. PASS
/// when `after` read every process's user and lists none of the pool user's; FAIL naming each
/// it lists; INFO when it couldn't read some process's user.
pub(crate) fn at_end(running: &[String], agent: u32, after: &[String], waited_ms: u128) -> Verdict {
    match pooled(running) {
        Ok((found, unknown)) if !found.iter().any(|p| p.pid == agent) => {
            return Verdict::Info(format!(
                "the harness's listing didn't see the agent ({agent}) as the pool user while \
                 the session ran ({unknown} processes' users unknown), so an empty listing at \
                 the end proves nothing"
            ));
        }
        Ok(_) => {}
        Err(why) => return Verdict::Fail(format!("while the session ran: {why}")),
    }
    let (found, unknown) = match pooled(after) {
        Ok(read) => read,
        Err(why) => return Verdict::Fail(format!("after the session ended: {why}")),
    };
    if !found.is_empty() {
        let left: Vec<&Pooled<'_>> = found.iter().collect();
        return Verdict::Fail(format!(
            "{waited_ms} ms after the session ended the pool user still runs: {}",
            named(&left)
        ));
    }
    let note = format!(
        "no process of the pool user {waited_ms} ms after the session ended (the agent {agent} \
         was seen while it ran)"
    );
    unreadable(after, unknown, &note).unwrap_or(Verdict::Pass(note))
}

/// `sweep-before-logon` (`within` `None`: gone by the agent's first line) and
/// `sweep-mid-session` (`within` [`MID_SESSION_MS`]). PASS when the benign process ran and is
/// gone (within `within` of its start); FAIL when it still ran; INFO when it never ran.
pub(crate) fn swept(lines: &[String], within: Option<u128>) -> Verdict {
    if let Some(why) = values(lines, "benign never-ran").first() {
        return Verdict::Info(format!(
            "the benign process outside the job never ran ({why}), so it proves nothing"
        ));
    }
    let Some(pid) = values(lines, "benign ran").first().map(|p| (*p).to_owned()) else {
        return Verdict::Fail(format!("no benign line: {lines:?}"));
    };
    let when = match within {
        None => "by the time the agent printed its first line".to_owned(),
        Some(ms) => format!("within {ms} ms"),
    };
    if let Some(ms) = values(lines, "benign alive").first() {
        return Verdict::Fail(format!(
            "the pool user's benign process {pid}, outside the session's job, still ran {when} \
             ({ms} ms after it started): no sweep ended it"
        ));
    }
    let Some(gone) = values(lines, "benign gone")
        .first()
        .map(|g| (*g).to_owned())
    else {
        return Verdict::Fail(format!("no benign gone or alive line: {lines:?}"));
    };
    let ms = gone
        .split_whitespace()
        .next()
        .and_then(|ms| ms.parse::<u128>().ok());
    match (ms, within) {
        (None, _) => Verdict::Fail(format!("an unreadable benign line: {gone:?}")),
        (Some(ms), Some(limit)) if ms > limit => Verdict::Fail(format!(
            "the benign process {pid} ended {ms} ms after it started, past {limit} ms"
        )),
        (Some(_), _) => Verdict::Pass(format!(
            "the pool user's benign process {pid}, outside the session's job, was gone {when} \
             ({gone})"
        )),
    }
}

/// `agent-token-config`, from `probe token`'s lines: PASS when the agent's restricting SIDs are
/// exactly the pool user, `S-1-5-12`, `S-1-5-32-545` and `S-1-1-0`, its integrity is Low and
/// every logon SID is deny-only (the values `whoami` asserts too); FAIL naming each that differs;
/// INFO when the token couldn't be read.
pub(crate) fn token_config(lines: &[String], pool_sid: &str) -> Verdict {
    if let Some(code) = values(lines, "token error").first() {
        return Verdict::Info(format!("the agent's token couldn't be read ({code})"));
    }
    if values(lines, "user").is_empty() {
        return Verdict::Info(format!("probe token printed no token: {lines:?}"));
    }
    let mut bad = Vec::new();
    let restricted: BTreeSet<&str> = values(lines, "restricted").into_iter().collect();
    let want: BTreeSet<&str> = [pool_sid, "S-1-5-12", "S-1-5-32-545", "S-1-1-0"].into();
    if restricted != want {
        bad.push(format!("restricting SIDs {restricted:?}, not {want:?}"));
    }
    let integrity = values(lines, "integrity");
    if integrity != ["S-1-16-4096"] {
        bad.push(format!("integrity {integrity:?}, not Low (S-1-16-4096)"));
    }
    let logon = logon_sids(lines);
    if logon.is_empty() {
        bad.push("no logon SID".to_owned());
    } else if logon.iter().any(|(_, a)| a & DENY_ONLY == 0) {
        bad.push(format!("a logon SID isn't deny-only: {logon:?}"));
    }
    if !bad.is_empty() {
        return Verdict::Fail(bad.join("; "));
    }
    Verdict::Pass(format!(
        "restricting SIDs {want:?}; Low; logon SID deny-only {logon:?}"
    ))
}

/// The logon SIDs among `probe token`'s `group` lines, with their attributes.
pub(crate) fn logon_sids(lines: &[String]) -> Vec<(&str, u32)> {
    values(lines, "group")
        .into_iter()
        .filter_map(|v| {
            let (sid, attributes) = v.split_once(' ')?;
            let hex = attributes.trim_start_matches("0x");
            Some((sid, u32::from_str_radix(hex, 16).ok()?))
        })
        .filter(|(_, attributes)| attributes & LOGON_ID == LOGON_ID)
        .collect()
}

/// `breakaway`, from the probe's lines and the harness's `breakaway our-job` line: PASS when
/// `CREATE_BREAKAWAY_FROM_JOB` failed with 5, or when its child is in the session's own
/// kill-on-close job. A child in some other job, or in none, fails.
pub(crate) fn breakaway(lines: &[String]) -> Result<String, String> {
    if let Some(code) = values(lines, "breakaway error").first() {
        if *code == DENIED {
            return Ok("CreateProcess with CREATE_BREAKAWAY_FROM_JOB gave 5".to_owned());
        }
        return Err(format!(
            "CreateProcess with CREATE_BREAKAWAY_FROM_JOB gave {code}, not 5"
        ));
    }
    let child = values(lines, "breakaway child")
        .first()
        .map(|v| (*v).to_owned())
        .ok_or_else(|| format!("no breakaway line: {lines:?}"))?;
    let killed = values(lines, "breakaway killed").join(" ");
    match values(lines, "breakaway our-job").first().copied() {
        Some("true") => Ok(format!(
            "CREATE_BREAKAWAY_FROM_JOB made a child still in the session's kill-on-close job \
             ({child}); killed ({killed})"
        )),
        Some("false") if child.ends_with("in-job true") => Err(format!(
            "the CREATE_BREAKAWAY_FROM_JOB child is in a job, but not the session's \
             kill-on-close job: {child}; killed ({killed})"
        )),
        Some("false") => Err(format!(
            "the CREATE_BREAKAWAY_FROM_JOB child left the job: {child}; killed ({killed})"
        )),
        Some(other) => Err(format!(
            "the breakaway child's job couldn't be checked: {other} ({child})"
        )),
        None => Err(format!(
            "the breakaway child's job wasn't checked ({child}); killed ({killed})"
        )),
    }
}

/// `wts-visibility`, from `probe wts`'s lines (`win_pool::wts_lines`): always INFO, naming what
/// a pool user's `WTSEnumerateProcessesW` returned next to its Toolhelp snapshot, or the error.
pub(crate) fn wts_visibility(lines: &[String]) -> Verdict {
    let toolhelp = match (
        values(lines, "wts toolhelp").first(),
        values(lines, "wts toolhelp-error").first(),
    ) {
        (Some(n), _) => format!("Toolhelp {n} processes"),
        (None, Some(code)) => format!("Toolhelp failed ({code})"),
        (None, None) => "no Toolhelp line".to_owned(),
    };
    if let Some(code) = values(lines, "wts enum-error").first() {
        return Verdict::Info(format!(
            "as the pool user, WTSEnumerateProcessesW failed with {code}; {toolhelp}"
        ));
    }
    let Some(counts) = values(lines, "wts enum").first().and_then(|rest| {
        let words: Vec<&str> = rest.split_whitespace().collect();
        match words.as_slice() {
            [total, "sid", sid, "own", own] => Some((*total, *sid, *own)),
            _ => None,
        }
    }) else {
        return Verdict::Info(format!("probe wts printed no wts enum line: {lines:?}"));
    };
    let (total, sid, own) = counts;
    let missing = values(lines, "wts missing").first().map_or_else(
        || "the Toolhelp processes WTS left out weren't counted".to_owned(),
        |rest| {
            let (n, names) = rest.split_once(' ').unwrap_or((rest, ""));
            format!("{n} Toolhelp processes not in WTS (first: {names})")
        },
    );
    Verdict::Info(format!(
        "as the pool user: {toolhelp}; WTSEnumerateProcessesW {total} entries, {sid} with a user \
         SID, {own} the pool user's; {missing}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        MID_SESSION_MS, Verdict, at_end, at_start, breakaway, swept, token_config, wts_visibility,
    };

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|l| (*l).to_owned()).collect()
    }

    fn fail_names(verdict: &Verdict, words: &[&str]) {
        let Verdict::Fail(why) = verdict else {
            panic!("not a FAIL: {verdict:?}");
        };
        for word in words {
            assert!(why.contains(word), "{why:?} doesn't name {word:?}");
        }
    }

    const POOL: &str = "S-1-5-21-1-1001";
    const RUNNING: [&str; 3] = [
        "pool 200 in-job=true clauseo-sandbox-run.exe",
        "pool 300 in-job=true s15_launch_vm.exe",
        "pool-unknown 0",
    ];

    #[test]
    fn at_start_passes_when_every_pool_process_is_in_the_job() {
        assert!(matches!(at_start(&lines(&RUNNING), 300), Verdict::Pass(_)));
    }

    #[test]
    fn at_start_fails_naming_a_pool_process_outside_the_job() {
        let mut outside = RUNNING.to_vec();
        outside.insert(2, "pool 400 in-job=false probe wait.exe");
        outside.insert(2, "pool 500 in-job=error:5 cmd.exe");
        fail_names(
            &at_start(&lines(&outside), 300),
            &["400 (probe wait.exe", "500 (cmd.exe"],
        );
        fail_names(&at_start(&lines(&["pool-error 5"]), 300), &["failed: 5"]);
    }

    #[test]
    fn at_start_is_info_when_the_listing_misses_the_agent() {
        let missed = lines(&[
            "pool 200 in-job=true clauseo-sandbox-run.exe",
            "pool-unknown 0",
        ]);
        assert!(matches!(at_start(&missed, 300), Verdict::Info(_)));
    }

    #[test]
    fn at_end_passes_only_on_an_empty_listing_after_a_listing_that_saw_the_agent() {
        let empty = lines(&["pool-unknown 0"]);
        assert!(matches!(
            at_end(&lines(&RUNNING), 300, &empty, 1_500),
            Verdict::Pass(_)
        ));
        let left = lines(&["pool 400 in-job=unchecked probe.exe", "pool-unknown 0"]);
        fail_names(
            &at_end(&lines(&RUNNING), 300, &left, 10_000),
            &["400 (probe.exe"],
        );
        let missed = lines(&["pool-unknown 0"]);
        assert!(matches!(at_end(&missed, 300, &empty, 0), Verdict::Info(_)));
    }

    #[test]
    fn an_unreadable_process_makes_either_listing_info_never_pass() {
        let mut start = RUNNING.to_vec();
        start.pop();
        start.extend([
            "pool-unreadable 800 csrss.exe",
            "pool-unreadable 900 lsass.exe",
        ]);
        start.push("pool-unknown 2");
        let Verdict::Info(why) = at_start(&lines(&start), 300) else {
            panic!("not INFO");
        };
        assert!(why.contains("2 processes (csrss.exe, lsass.exe)"), "{why}");
        let after = lines(&["pool-unreadable 800 csrss.exe", "pool-unknown 1"]);
        let Verdict::Info(why) = at_end(&lines(&RUNNING), 300, &after, 1_000) else {
            panic!("not INFO");
        };
        assert!(why.contains("1 processes (csrss.exe)"), "{why}");
        // A process outside the job still fails, whatever else was unreadable.
        start.insert(0, "pool 400 in-job=false probe.exe");
        fail_names(&at_start(&lines(&start), 300), &["400 (probe.exe"]);
        // No count at all is no listing.
        fail_names(&at_end(&lines(&RUNNING), 300, &[], 0), &["pool-unknown"]);
    }

    #[test]
    fn sweep_before_logon_needs_the_benign_process_gone_at_the_first_line() {
        let gone = lines(&["benign ran 600", "benign gone 0 exit 0x1"]);
        assert!(matches!(swept(&gone, None), Verdict::Pass(_)));
        let alive = lines(&["benign ran 600", "benign alive 2300"]);
        fail_names(&swept(&alive, None), &["600", "still ran"]);
        let never = lines(&["benign never-ran CreateProcessWithLogonW(ClauseoSb01) gave 1326"]);
        assert!(matches!(swept(&never, None), Verdict::Info(_)));
    }

    #[test]
    fn sweep_mid_session_needs_the_benign_process_gone_within_45_s() {
        let within = Some(MID_SESSION_MS);
        let gone = lines(&["benign ran 600", "benign gone 31000 exit 0x1"]);
        assert!(matches!(swept(&gone, within), Verdict::Pass(_)));
        let late = lines(&["benign ran 600", "benign gone 46000 exit 0x1"]);
        fail_names(&swept(&late, within), &["46000"]);
        let alive = lines(&["benign ran 600", "benign alive 45000"]);
        fail_names(&swept(&alive, within), &["600"]);
        assert!(matches!(
            swept(&lines(&["benign never-ran no pid line"]), within),
            Verdict::Info(_)
        ));
    }

    const TOKEN: [&str; 8] = [
        "user S-1-5-21-1-1001",
        "integrity S-1-16-4096",
        "restricted S-1-5-21-1-1001",
        "restricted S-1-5-12",
        "restricted S-1-5-32-545",
        "restricted S-1-1-0",
        "group S-1-5-5-0-1234 0xc0000010",
        "group S-1-5-32-545 0x7",
    ];

    #[test]
    fn agent_token_config_asserts_restricting_sids_low_and_a_deny_only_logon_sid() {
        assert!(matches!(
            token_config(&lines(&TOKEN), POOL),
            Verdict::Pass(_)
        ));
        let mut changed = TOKEN;
        for (at, line) in [
            (1, "integrity S-1-16-8192"),
            (5, "restricted S-1-5-11"),
            (6, "group S-1-5-5-0-1234 0xc0000007"),
        ] {
            if let Some(slot) = changed.get_mut(at) {
                *slot = line;
            }
        }
        fail_names(
            &token_config(&lines(&changed), POOL),
            &["restricting SIDs", "integrity", "deny-only"],
        );
        let no_logon = lines(TOKEN.get(..6).unwrap_or_default());
        fail_names(&token_config(&no_logon, POOL), &["no logon SID"]);
        assert!(matches!(
            token_config(&lines(&["token error 5"]), POOL),
            Verdict::Info(_)
        ));
    }

    #[test]
    fn breakaway_passes_on_5_or_a_child_in_our_job() {
        assert!(breakaway(&lines(&["breakaway error 5"])).is_ok());
        assert!(breakaway(&lines(&["breakaway error 87"])).is_err());
        let ours = lines(&[
            "breakaway child 700 in-job true",
            "breakaway killed 0",
            "breakaway our-job true",
        ]);
        assert!(breakaway(&ours).is_ok());
    }

    #[test]
    fn breakaway_fails_on_a_child_in_a_job_that_isnt_ours() {
        let other = lines(&[
            "breakaway child 700 in-job true",
            "breakaway killed 0",
            "breakaway our-job false",
        ]);
        let why = breakaway(&other).unwrap_err();
        assert!(why.contains("not the session's kill-on-close job"), "{why}");
        let none = lines(&[
            "breakaway child 700 in-job false",
            "breakaway our-job false",
        ]);
        assert!(breakaway(&none).unwrap_err().contains("left the job"));
        let unchecked = lines(&["breakaway child 700 in-job true", "breakaway killed 0"]);
        assert!(breakaway(&unchecked).is_err());
        let failed = lines(&[
            "breakaway child 700 in-job true",
            "breakaway our-job error OpenProcess gave 5",
        ]);
        assert!(breakaway(&failed).is_err());
    }

    fn info_names(verdict: &Verdict, words: &[&str]) {
        let Verdict::Info(text) = verdict else {
            panic!("not INFO: {verdict:?}");
        };
        for word in words {
            assert!(text.contains(word), "{text:?} doesn't name {word:?}");
        }
    }

    #[test]
    fn wts_visibility_is_info_naming_the_counts() {
        let seen = lines(&[
            "wts toolhelp 120",
            "wts enum 41 sid 41 own 3",
            "wts missing 79 lsass.exe,csrss.exe",
        ]);
        info_names(
            &wts_visibility(&seen),
            &[
                "Toolhelp 120",
                "41 entries",
                "41 with a user SID",
                "3 the pool user's",
                "79",
            ],
        );
        info_names(&wts_visibility(&seen), &["lsass.exe,csrss.exe"]);
    }

    #[test]
    fn wts_visibility_is_info_on_an_error_or_no_lines() {
        let failed = lines(&["wts toolhelp 120", "wts enum-error 5"]);
        info_names(&wts_visibility(&failed), &["failed with 5", "Toolhelp 120"]);
        let no_toolhelp = lines(&["wts toolhelp-error 24", "wts enum 3 sid 0 own 0"]);
        info_names(
            &wts_visibility(&no_toolhelp),
            &["Toolhelp failed (24)", "3 entries"],
        );
        info_names(&wts_visibility(&[]), &["no wts enum line"]);
        info_names(
            &wts_visibility(&lines(&["wts enum 3 sid"])),
            &["no wts enum line"],
        );
    }
}
