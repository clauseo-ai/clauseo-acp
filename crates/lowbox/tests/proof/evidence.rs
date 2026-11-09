//! The helper's report lines, parsed, printed and checked against the row's assertions, and
//! [`redact`], which every helper or chrome text passes through before the owner shows it.

use std::collections::BTreeMap;

use super::helper::MARK;

/// One report line: its kind (the first word) and its `key=value` fields.
#[derive(Debug, Clone)]
pub(crate) struct Line {
    /// The first word: `fact`, `station`, `exit`, …
    pub(crate) kind: String,
    /// The fields.
    pub(crate) fields: BTreeMap<String, String>,
    /// The line after the mark, as printed.
    pub(crate) text: String,
}

impl Line {
    /// A field, or `-`.
    pub(crate) fn get(&self, key: &str) -> &str {
        self.fields.get(key).map_or("-", String::as_str)
    }
}

/// The report lines among the helper's output (the test harness's own lines are skipped).
pub(crate) fn parse(output: &[String]) -> Vec<Line> {
    output
        .iter()
        .filter_map(|raw| {
            let text = &raw[raw.find(MARK)? + MARK.len()..];
            let mut words = text.split_whitespace();
            let kind = words.next()?.to_owned();
            let fields = words
                .filter_map(|w| w.split_once('='))
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect();
            Some(Line {
                kind,
                fields,
                text: text.trim().to_owned(),
            })
        })
        .collect()
}

/// The PNG signature every screenshot starts with.
pub(crate) const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
/// The smallest screenshot the row accepts, in bytes (an 800 by 600 page is far more).
const SHOT_MIN: u64 = 1024;

/// `text` with every package SID (`S-1-15-2-` and the digits and dashes after it) cut to
/// `S-1-15-2-…`.
pub(crate) fn redact(text: &str) -> String {
    const PREFIX: &str = "S-1-15-2-";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.to_ascii_uppercase().find(PREFIX) {
        out.push_str(&rest[..at]);
        out.push_str("S-1-15-2-…");
        rest =
            rest[at + PREFIX.len()..].trim_start_matches(|c: char| c.is_ascii_digit() || c == '-');
    }
    out.push_str(rest);
    out
}

/// What the owner knows besides the report.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    /// The private station's name.
    pub(crate) station: String,
    /// The screenshot's size in bytes (0 when missing).
    pub(crate) shot_bytes: u64,
    /// Whether the screenshot starts with [`PNG_SIGNATURE`].
    pub(crate) shot_png: bool,
    /// How many page answers the proxy wrote in full.
    pub(crate) proxy_delivered: u32,
    /// The helper's exit code, `None` when it had to be killed.
    pub(crate) helper_exit: Option<u32>,
}

/// The facts of every live process (`open=ok`) with `role`.
fn with_role<'a>(lines: &'a [Line], role: &str) -> Vec<&'a Line> {
    lines
        .iter()
        .filter(|l| l.kind == "fact" && l.get("role") == role && l.get("open") == "ok")
        .collect()
}

/// The first `kind` line's `key`.
fn first<'a>(lines: &'a [Line], kind: &str, key: &str) -> Option<&'a str> {
    lines.iter().find(|l| l.kind == kind).map(|l| l.get(key))
}

/// Every assertion the row makes; the failures, each a sentence.
pub(crate) fn check(lines: &[Line], context: &Context) -> Vec<String> {
    let mut problems = check_station(lines, context);
    problems.extend(check_processes(lines));
    problems.extend(check_probe(lines));
    problems.extend(check_exits(lines));
    problems.extend(check_cdp(lines));
    problems.extend(check_listeners(lines));
    if context.shot_bytes <= SHOT_MIN {
        problems.push(format!(
            "shot.png is {} bytes, not over {SHOT_MIN}",
            context.shot_bytes
        ));
    }
    if !context.shot_png {
        problems.push("shot.png does not start with the PNG signature".to_owned());
    }
    if context.proxy_delivered == 0 {
        problems.push(format!(
            "the proxy never wrote a whole answer to GET {}",
            super::proxy::URL
        ));
    }
    match context.helper_exit {
        Some(0) => {}
        Some(code) => problems.push(format!("the helper exited with {code:#x}, not 0")),
        None => problems.push("the helper did not exit in time and was killed".to_owned()),
    }
    problems
}

/// The helper's station: the private one by its own `UOI_NAME`, and it opened by name there.
fn check_station(lines: &[Line], context: &Context) -> Vec<String> {
    let mut problems = Vec::new();
    let station = first(lines, "station", "name");
    match station {
        None => problems.push("the helper never reported its window station".to_owned()),
        Some(name) if name.eq_ignore_ascii_case("WinSta0") => {
            problems.push("the helper ran on WinSta0, not the private station".to_owned());
        }
        Some(name) if name != context.station => problems.push(format!(
            "the helper ran on station {name}, not the private {}",
            context.station
        )),
        Some(_) => {}
    }
    let opens = first(lines, "station", "opens").unwrap_or("-");
    if opens != "1" {
        problems.push(format!(
            "the helper could not open its station by name (opens={opens})"
        ));
    }
    problems
}

/// Each role chrome must run, with the integrity RID its token must have.
const ROLES: [(&str, u32); 4] = [
    ("browser", LOW),
    ("renderer", UNTRUSTED),
    ("gpu", LOW),
    ("network", LOW),
];
/// The integrity RID of Untrusted.
const UNTRUSTED: u32 = 0x0000;
/// The integrity RID of Low.
const LOW: u32 = 0x1000;
/// The one privilege a `LowBox` token may keep (the crate strips every other one).
const KEPT_PRIVILEGE: &str = "SeChangeNotifyPrivilege";

/// The integrity name of `rid`, for the messages.
fn integrity_name(rid: u32) -> &'static str {
    if rid == UNTRUSTED { "Untrusted" } else { "Low" }
}

/// A fact's `integrity` as a RID, when it is one.
fn integrity_of(fact: &Line) -> Option<u32> {
    u32::from_str_radix(fact.get("integrity").strip_prefix("0x")?, 16).ok()
}

/// Chrome's processes: chrome itself, a renderer, the GPU process and the network service, each
/// found live with its role's integrity (Low; Untrusted for renderers). Every live process of
/// chrome's tree, those four roles and any other, is in an `AppContainer` whose package is this
/// one, holds no capability and no privilege but `SeChangeNotifyPrivilege`, and is no higher
/// than Low. Every process not gone was opened with its token, as the guard needs.
fn check_processes(lines: &[Line]) -> Vec<String> {
    let mut problems = Vec::new();
    for (role, rid) in ROLES {
        let live = with_role(lines, role);
        if live.is_empty() {
            problems.push(format!("no live {role} process was found"));
        }
        for fact in live {
            if integrity_of(fact) != Some(rid) {
                problems.push(format!(
                    "{role} pid {} has integrity {}, not {} ({rid:#06x})",
                    fact.get("pid"),
                    fact.get("integrity"),
                    integrity_name(rid)
                ));
            }
        }
    }
    let live = lines
        .iter()
        .filter(|l| l.kind == "fact" && l.get("open") == "ok");
    for fact in live {
        problems.extend(check_token(fact));
    }
    for f in lines
        .iter()
        .filter(|l| l.kind == "fact" && l.get("open") != "gone")
    {
        if f.get("open") != "ok" || f.get("token") != "ok" {
            problems.push(format!(
                "B_s could not open {} pid {} (process {}, token {}): the guard would refuse",
                f.get("role"),
                f.get("pid"),
                f.get("open"),
                f.get("token")
            ));
        }
    }
    problems
}

/// One live process's token: this package's `AppContainer`, no capability, no privilege but
/// [`KEPT_PRIVILEGE`], no higher than Low.
fn check_token(fact: &Line) -> Vec<String> {
    let mut problems = Vec::new();
    let (role, pid) = (fact.get("role"), fact.get("pid"));
    if fact.get("appcontainer") != "1" || fact.get("this_package") != "1" {
        problems.push(format!(
            "{role} pid {pid} is not in this package's AppContainer (appcontainer={}, \
             this_package={})",
            fact.get("appcontainer"),
            fact.get("this_package")
        ));
    }
    if fact.get("caps") != "0" {
        problems.push(format!(
            "{role} pid {pid} has capabilities={}, not 0",
            fact.get("caps")
        ));
    }
    let privs = fact.get("privs");
    let extra: Vec<&str> = match privs {
        "none" => Vec::new(),
        "-" => vec!["(not read)"],
        list => list.split(',').filter(|p| *p != KEPT_PRIVILEGE).collect(),
    };
    if !extra.is_empty() {
        problems.push(format!(
            "{role} pid {pid} holds privileges {privs}: only {KEPT_PRIVILEGE} may remain (extra: \
             {})",
            extra.join(",")
        ));
    }
    if integrity_of(fact).is_none_or(|rid| rid > LOW) {
        problems.push(format!(
            "{role} pid {pid} has integrity {}, above Low or unread",
            fact.get("integrity")
        ));
    }
    problems
}

/// The probe's opens package B must be refused: A's namespace and each of A's folders.
const DENIED: [&str; 10] = [
    "a-dir",
    "a-pipes",
    "a-profile-list",
    "a-profile-read",
    "a-staging-list",
    "a-staging-read",
    "a-tmp-list",
    "a-tmp-read",
    "a-shell-list",
    "a-shell-read",
];
/// The probe's positive controls, which must open: B's own namespace and the shared folder.
const ALLOWED: [&str; 4] = ["b-dir", "b-pipes", "shared-list", "shared-read"];
/// `STATUS_ACCESS_DENIED`.
const NT_DENIED: u32 = 0xC000_0022;
/// `ERROR_ACCESS_DENIED`.
const WIN32_DENIED: u32 = 5;

/// The probe's `code` for `target` and its `api`, if it reported one.
fn probe_result<'a>(lines: &'a [Line], target: &str) -> Option<(&'a str, Option<u32>)> {
    let line = lines
        .iter()
        .find(|l| l.kind == "probe" && l.get("target") == target)?;
    let code = line
        .get("code")
        .strip_prefix("0x")
        .and_then(|hex| u32::from_str_radix(hex, 16).ok());
    Some((line.get("api"), code))
}

/// The negative check: package B's probe exited 0, every open of A's was refused with access
/// denied (not some other error, which would prove nothing), and every positive control opened.
fn check_probe(lines: &[Line]) -> Vec<String> {
    let mut problems = Vec::new();
    match lines.iter().find(|l| l.kind == "probe-exit") {
        None => problems.push("the helper never reported the probe's exit".to_owned()),
        Some(exit) if exit.get("exited") != "1" || exit.get("code") != "0x0" => {
            problems.push(format!(
                "package B's probe did not exit 0 in time: {}",
                exit.text
            ));
        }
        Some(_) => {}
    }
    for target in DENIED {
        match probe_result(lines, target) {
            None => problems.push(format!("no probe result for {target}")),
            Some(("nt", Some(NT_DENIED)) | ("win32", Some(WIN32_DENIED))) => {}
            Some((api, Some(0))) => problems.push(format!(
                "package B opened package A's {target} ({api} code 0): the isolation failed"
            )),
            Some((api, code)) => problems.push(format!(
                "package B's open of A's {target} failed with {api} code {code:#x?}, not access \
                 denied"
            )),
        }
    }
    for target in ALLOWED {
        match probe_result(lines, target) {
            None => problems.push(format!("no probe result for the control {target}")),
            Some((_, Some(0))) => {}
            Some((api, code)) => problems.push(format!(
                "the positive control {target} failed with {api} code {code:#x?}: the probe \
                 cannot tell a refusal from a broken open"
            )),
        }
    }
    problems
}

/// Chrome exited within the helper's wait, and every exit code the helper held a handle for was
/// read (the values are printed, not judged).
fn check_exits(lines: &[Line]) -> Vec<String> {
    let mut problems = Vec::new();
    match lines.iter().find(|l| l.kind == "chrome") {
        None => problems.push("the helper never reported chrome's exit".to_owned()),
        Some(chrome) => {
            if chrome.get("exited") != "1" {
                problems.push(format!(
                    "chrome did not exit within the helper's {} s wait after Browser.close",
                    super::helper::CHROME_EXIT_MS / 1000
                ));
            }
            if chrome.get("code") == "-" {
                problems.push("chrome's exit code was not read".to_owned());
            }
        }
    }
    for exit in lines.iter().filter(|l| l.kind == "exit") {
        if exit.get("code") == "running" {
            problems.push(format!(
                "the exit code of {} pid {} was not read: still running",
                exit.get("role"),
                exit.get("pid")
            ));
        }
    }
    problems
}

/// The CDP steps that must each print an `ok` line, as their lines start.
const CDP_STEPS: [&str; 8] = [
    "cdp method=Browser.getVersion ok",
    "cdp method=Target.createTarget ok",
    "cdp method=Target.attachToTarget ok",
    "cdp method=Page.enable ok",
    "cdp method=Page.navigate ok",
    "cdp event=Page.loadEventFired ok",
    "cdp method=Page.captureScreenshot ok",
    "cdp method=Browser.close ok",
];

/// Every CDP step succeeded; a failed one is reported with its method and CDP's message.
fn check_cdp(lines: &[Line]) -> Vec<String> {
    let cdp: Vec<&Line> = lines.iter().filter(|l| l.kind == "cdp").collect();
    let mut problems: Vec<String> = cdp
        .iter()
        .filter(|l| l.text.contains(" failed: "))
        .map(|l| format!("CDP step failed: {}", l.text))
        .collect();
    for step in CDP_STEPS {
        if !cdp.iter().any(|l| l.text.starts_with(step)) {
            problems.push(format!("no `{step}` line from the helper"));
        }
    }
    problems
}

/// No process of chrome's job listened on a TCP port, at either look (`inspect`, `close`).
/// Each look must have counted chrome's processes, chrome itself among them: a look that found
/// none would pass with nothing checked.
fn check_listeners(lines: &[Line]) -> Vec<String> {
    let listen: Vec<&Line> = lines.iter().filter(|l| l.kind == "listen").collect();
    let chrome = first(lines, "started", "pid").unwrap_or("-");
    let mut problems = Vec::new();
    for stage in ["inspect", "close"] {
        let counted: Vec<&str> = listen
            .iter()
            .filter(|l| l.get("stage") == stage && l.fields.contains_key("pid"))
            .map(|l| l.get("pid"))
            .collect();
        let pids = listen
            .iter()
            .find(|l| l.get("stage") == stage && l.fields.contains_key("pids"))
            .map(|l| l.get("pids"));
        match pids {
            None => problems.push(format!("no TCP listener count at the {stage} look")),
            Some(n) if n == "0" || counted.is_empty() => problems.push(format!(
                "the {stage} look found no chrome process (pids={n}): nothing was checked"
            )),
            Some(_) if !counted.contains(&chrome) => problems.push(format!(
                "the {stage} look did not count chrome itself (pid {chrome})"
            )),
            Some(_) => {}
        }
    }
    for l in &listen {
        if l.fields.contains_key("error") {
            problems.push(format!("the listener count failed: {}", l.text));
        }
        if l.fields.contains_key("pid") && l.get("listeners") != "0" {
            problems.push(format!(
                "chrome pid {} listens on {} TCP port(s) at the {} look",
                l.get("pid"),
                l.get("listeners"),
                l.get("stage")
            ));
        }
    }
    problems
}

/// The evidence as the CI log shows it.
pub(crate) fn summary(lines: &[Line], context: &Context) -> Vec<String> {
    let mut out = vec![
        format!(
            "evidence: chrome machine {}",
            lines
                .iter()
                .find(|l| l.kind == "machine")
                .map_or("(not reported)", |l| l.text.as_str())
        ),
        format!(
            "evidence: helper station {} (private station {}, opens by name from the helper: \
             {}), desktop {}",
            first(lines, "station", "name").unwrap_or("(not reported)"),
            context.station,
            first(lines, "station", "opens").unwrap_or("(not reported)"),
            first(lines, "desktop", "name").unwrap_or("(not reported)")
        ),
        format!(
            "evidence: proxy page answers written {}, shot.png {} bytes (PNG signature: {}), \
             helper exit {:?}",
            context.proxy_delivered, context.shot_bytes, context.shot_png, context.helper_exit
        ),
    ];
    for l in lines {
        if matches!(
            l.kind.as_str(),
            "fact"
                | "exit"
                | "started"
                | "inspect"
                | "inspected"
                | "chrome"
                | "handles"
                | "probe-start"
                | "probe"
                | "probe-done"
                | "probe-exit"
                | "snapshots"
                | "stderr"
                | "cdp"
                | "listen"
        ) {
            out.push(format!("evidence: {}", l.text));
        }
    }
    out
}

#[test]
fn redact_cuts_every_package_sid() {
    assert_eq!(
        redact(r"x \Sessions\1\AppContainerNamedObjects\S-1-15-2-12-345-6 y s-1-15-2-7"),
        r"x \Sessions\1\AppContainerNamedObjects\S-1-15-2-… y S-1-15-2-…"
    );
    assert_eq!(redact("S-1-5-21-1-2 S-1-15-3-4"), "S-1-5-21-1-2 S-1-15-3-4");
}
