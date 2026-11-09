//! The helper's report lines, parsed, printed and checked against the row's assertions.

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

/// What the owner knows besides the report.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    /// The private station's name.
    pub(crate) station: String,
    /// Whether it opened by name after the run.
    pub(crate) station_exists: bool,
    /// The screenshot's size in bytes (0 when missing).
    pub(crate) shot_bytes: u64,
    /// How many page requests the proxy saw.
    pub(crate) proxy_seen: u32,
    /// The helper's exit code, `None` when it had to be killed.
    pub(crate) helper_exit: Option<u32>,
}

/// The facts of every process with `role`.
fn with_role<'a>(lines: &'a [Line], role: &str) -> Vec<&'a Line> {
    lines
        .iter()
        .filter(|l| l.kind == "fact" && l.get("role") == role)
        .collect()
}

/// The first `kind` line's `key`.
fn first<'a>(lines: &'a [Line], kind: &str, key: &str) -> Option<&'a str> {
    lines.iter().find(|l| l.kind == kind).map(|l| l.get(key))
}

/// Every assertion the row makes; the failures, each a sentence.
pub(crate) fn check(lines: &[Line], context: &Context) -> Vec<String> {
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
    if !context.station_exists {
        problems.push(format!(
            "the station {} did not open by name",
            context.station
        ));
    }
    let facts: Vec<&Line> = lines.iter().filter(|l| l.kind == "fact").collect();
    if with_role(lines, "browser").is_empty() {
        problems.push("no fact for chrome itself (role=browser)".to_owned());
    }
    let renderers = with_role(lines, "renderer");
    if renderers.is_empty() {
        problems.push("no renderer process was found".to_owned());
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
        problems.push("no GPU process was found".to_owned());
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
        problems.push("no network-service process was found".to_owned());
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
    if context.shot_bytes == 0 {
        problems.push("shot.png is missing or empty".to_owned());
    }
    if context.proxy_seen == 0 {
        problems.push(format!("the proxy never saw GET {}", super::proxy::URL));
    }
    match context.helper_exit {
        Some(0) => {}
        Some(code) => problems.push(format!("the helper exited with {code:#x}, not 0")),
        None => problems.push("the helper did not exit in time and was killed".to_owned()),
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
            "evidence: helper station {} (private station {}, opens by name: {}), desktop {}",
            first(lines, "station", "name").unwrap_or("(not reported)"),
            context.station,
            context.station_exists,
            first(lines, "desktop", "name").unwrap_or("(not reported)")
        ),
        format!(
            "evidence: proxy page requests {}, shot.png {} bytes, helper exit {:?}",
            context.proxy_seen, context.shot_bytes, context.helper_exit
        ),
    ];
    for l in lines {
        if matches!(
            l.kind.as_str(),
            "fact" | "exit" | "started" | "inspect" | "inspected" | "chrome" | "handles"
        ) {
            out.push(format!("evidence: {}", l.text));
        }
    }
    out
}
