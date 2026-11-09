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
    problems.extend(check_exits(lines));
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

/// Chrome's processes: one live renderer (Untrusted), GPU process (Low) and network service (in
/// this package) at least, and every live one opened with its token, as the guard needs.
fn check_processes(lines: &[Line]) -> Vec<String> {
    let mut problems = Vec::new();
    let facts: Vec<&Line> = lines
        .iter()
        .filter(|l| l.kind == "fact" && l.get("open") != "gone")
        .collect();
    if with_role(lines, "browser").is_empty() {
        problems.push("no fact for chrome itself (role=browser)".to_owned());
    }
    let renderers = with_role(lines, "renderer");
    if renderers.is_empty() {
        problems.push("no live renderer process was found".to_owned());
    }
    for r in &renderers {
        if r.get("integrity") != "0x0000" {
            problems.push(format!(
                "renderer pid {} has integrity {}, not Untrusted (0x0000)",
                r.get("pid"),
                r.get("integrity")
            ));
        }
    }
    let gpus = with_role(lines, "gpu");
    if gpus.is_empty() {
        problems.push("no live GPU process was found".to_owned());
    }
    for g in &gpus {
        if g.get("integrity") != "0x1000" {
            problems.push(format!(
                "GPU process pid {} has integrity {}, not Low (0x1000)",
                g.get("pid"),
                g.get("integrity")
            ));
        }
    }
    let networks = with_role(lines, "network");
    if networks.is_empty() {
        problems.push("no live network-service process was found".to_owned());
    }
    for n in &networks {
        if n.get("appcontainer") != "1" || n.get("this_package") != "1" {
            problems.push(format!(
                "network service pid {} is not in this package's AppContainer (appcontainer={}, \
                 this_package={})",
                n.get("pid"),
                n.get("appcontainer"),
                n.get("this_package")
            ));
        }
    }
    for f in &facts {
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

/// Chrome exited within the helper's wait, and every exit code the helper held a handle for was
/// read (the values are printed, not judged).
fn check_exits(lines: &[Line]) -> Vec<String> {
    let mut problems = Vec::new();
    match lines.iter().find(|l| l.kind == "chrome") {
        None => problems.push("the helper never reported chrome's exit".to_owned()),
        Some(chrome) => {
            if chrome.get("exited") != "1" {
                problems.push("chrome did not exit within the helper's 90 s wait".to_owned());
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
                | "snapshots"
                | "stderr"
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
