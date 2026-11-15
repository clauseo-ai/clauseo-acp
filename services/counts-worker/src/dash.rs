//! `/dash`: one server-rendered page with six charts as inline SVG, no script and no asset from
//! anywhere else, served under `Content-Security-Policy: default-src 'none'; style-src
//! 'unsafe-inline'`. The charts read only the `v_*` views, through the first four statements of
//! `dash.sql` (one D1 batch), so every number on them has passed the under-5 rule. Below them,
//! the cron's health: periods that should have been filled and were not, and the last 30 days
//! of `cron_status`. Those name no install and count none.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use clauseo_counts::schema::{OnboardingStep, Took, Week};
use serde_json::Value;

/// The page's security policy (S23).
pub(crate) const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'";

/// The four statements, in the order [`Data`] holds their rows.
const QUERIES: &str = include_str!("dash.sql");

/// The statements of `dash.sql`: its lines less comments, split at each `;`.
pub(crate) fn statements() -> Vec<String> {
    let mut code = String::new();
    for line in QUERIES
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
    {
        code.push_str(line);
        code.push('\n');
    }
    code.split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The rows of the four statements.
#[derive(Debug, Default)]
pub(crate) struct Data {
    /// `v_active`: `period`, `start`, `installs`.
    pub(crate) active: Vec<Value>,
    /// `v_cohort_week`, the last 12 cohorts: `first_week`, `week`, `installs`.
    pub(crate) cohorts: Vec<Value>,
    /// `v_daily`, summed: `event`, `bucket`, `on_day`, `count`, `installs`.
    pub(crate) daily: Vec<Value>,
    /// `v_tally`, the last 12 weeks: `week`, `version`, `event`, `count`.
    pub(crate) tally: Vec<Value>,
    /// Final weeks and months with reports but no fill: `period`, `start`.
    pub(crate) unfilled: Vec<Value>,
    /// `cron_status`, the last 30 days: `step`, `day`, `ok`, `failed`.
    pub(crate) cron: Vec<Value>,
}

const WIDTH: f64 = 640.0;
const HEIGHT: f64 = 140.0;
const COLOURS: [&str; 4] = ["#2563eb", "#d97706", "#059669", "#7c3aed"];

const STYLE: &str = "body{font:14px/1.4 system-ui,sans-serif;margin:2em auto;max-width:44em;\
color:#111}h1{font-size:1.4em}h2{font-size:1.1em;margin-top:2em}svg{display:block;\
background:#f8fafc}table{border-collapse:collapse;font-size:12px}td,th{padding:2px 6px;\
text-align:right;border:1px solid #e2e8f0}.note{color:#555;font-size:12px}\
.alert{color:#b91c1c}";

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}

fn number(row: &Value, key: &str) -> f64 {
    row.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// `value` with HTML's five special characters escaped.
pub(crate) fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn short(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

/// A bar chart of `points`, each bar titled with its label and value.
/// A count of bars or points as an `f64`, exactly: a chart holds far fewer than `u32::MAX`.
fn as_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

fn bars(points: &[(String, f64)]) -> String {
    if points.is_empty() {
        return "<p class=\"note\">No group of 5 or more yet.</p>".to_owned();
    }
    let max = points.iter().map(|(_, v)| *v).fold(1.0, f64::max);
    let step = WIDTH / as_f64(points.len());
    let mut svg = svg_open();
    for (i, (label, value)) in points.iter().enumerate() {
        let x = step * as_f64(i);
        let h = (HEIGHT - 20.0) * value / max;
        write!(
            svg,
            "<rect x=\"{x:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{h:.1}\" fill=\"{}\">\
             <title>{}: {}</title></rect>",
            HEIGHT - 15.0 - h,
            (step - 1.0).max(1.0),
            COLOURS[0],
            escape(label),
            short(*value),
        )
        .unwrap_or_default();
    }
    axis(&mut svg, &points[0].0, &points[points.len() - 1].0, max);
    svg.push_str("</svg>");
    svg
}

/// Lines over the shared `labels`, one per series; a missing point breaks nothing, it is
/// left out.
fn lines(labels: &[String], series: &[(String, Vec<Option<f64>>)]) -> String {
    if labels.is_empty() || series.is_empty() {
        return "<p class=\"note\">No group of 5 or more yet.</p>".to_owned();
    }
    let max = series
        .iter()
        .flat_map(|(_, values)| values.iter().flatten())
        .copied()
        .fold(0.1, f64::max);
    let step = WIDTH / as_f64(labels.len().max(2) - 1);
    let mut svg = svg_open();
    for (n, (name, values)) in series.iter().enumerate() {
        let colour = COLOURS[n % COLOURS.len()];
        let points: Vec<String> = values
            .iter()
            .enumerate()
            .filter_map(|(i, value)| {
                let x = step * as_f64(i);
                value.map(|v| format!("{x:.1},{:.1}", HEIGHT - 15.0 - (HEIGHT - 20.0) * v / max))
            })
            .collect();
        write!(
            svg,
            "<polyline fill=\"none\" stroke=\"{colour}\" stroke-width=\"2\" points=\"{}\">\
             <title>{}</title></polyline><text x=\"{}\" y=\"{}\" font-size=\"11\" \
             fill=\"{colour}\">{}</text>",
            points.join(" "),
            escape(name),
            WIDTH - 90.0,
            12.0 + 13.0 * f64::from(u8::try_from(n).unwrap_or(0)),
            escape(name),
        )
        .unwrap_or_default();
    }
    axis(&mut svg, &labels[0], &labels[labels.len() - 1], max);
    svg.push_str("</svg>");
    svg
}

fn svg_open() -> String {
    format!(
        "<svg viewBox=\"0 0 {WIDTH} {HEIGHT}\" \
         width=\"{WIDTH}\" height=\"{HEIGHT}\" role=\"img\">"
    )
}

fn axis(svg: &mut String, first: &str, last: &str, max: f64) {
    write!(
        svg,
        "<text x=\"2\" y=\"{}\" font-size=\"10\">{}</text><text x=\"{}\" y=\"{}\" \
         font-size=\"10\" text-anchor=\"end\">{}</text><text x=\"2\" y=\"10\" \
         font-size=\"10\">{}</text>",
        HEIGHT - 3.0,
        escape(first),
        WIDTH - 2.0,
        HEIGHT - 3.0,
        escape(last),
        short(max),
    )
    .unwrap_or_default();
}

/// DAU, WAU and MAU from `v_active`.
fn active(data: &Data) -> String {
    let mut out = String::new();
    for (period, name) in [("day", "Daily"), ("week", "Weekly"), ("month", "Monthly")] {
        let points: Vec<(String, f64)> = data
            .active
            .iter()
            .filter(|row| text(row, "period") == period)
            .map(|row| (text(row, "start").to_owned(), number(row, "installs")))
            .collect();
        write!(out, "<h3>{name} active installs</h3>{}", bars(&points)).unwrap_or_default();
    }
    out
}

/// The last 12 weekly cohorts: installs active each week after their first, with the share of
/// the first week's installs as the shade.
fn cohorts(data: &Data) -> String {
    let mut table: BTreeMap<&str, BTreeMap<i64, f64>> = BTreeMap::new();
    let mut span = 0;
    for row in &data.cohorts {
        let (Ok(first), Ok(week)) = (
            text(row, "first_week").parse::<Week>(),
            text(row, "week").parse::<Week>(),
        ) else {
            continue;
        };
        let since = week.weeks_since(first);
        span = span.max(since);
        table
            .entry(text(row, "first_week"))
            .or_default()
            .insert(since, number(row, "installs"));
    }
    let span = span.min(12);
    if table.is_empty() {
        return "<p class=\"note\">No cohort of 5 or more yet.</p>".to_owned();
    }
    let mut out = String::from("<table><tr><th>first week</th>");
    for since in 0..=span {
        write!(out, "<th>+{since}</th>").unwrap_or_default();
    }
    out.push_str("</tr>");
    for (first, weeks) in &table {
        let start = weeks.get(&0).copied().unwrap_or(0.0);
        write!(out, "<tr><th>{}</th>", escape(first)).unwrap_or_default();
        for since in 0..=span {
            match weeks.get(&since) {
                Some(installs) => {
                    let share = if start > 0.0 {
                        (installs / start).min(1.0)
                    } else {
                        0.0
                    };
                    write!(
                        out,
                        "<td style=\"background:rgba(37,99,235,{share:.2})\">{}</td>",
                        short(*installs)
                    )
                    .unwrap_or_default();
                }
                None => out.push_str("<td></td>"),
            }
        }
        out.push_str("</tr>");
    }
    out.push_str("</table>");
    out
}

/// Questions per active day, per agent: each day's `question_sent` counts over its reports.
fn questions(data: &Data) -> String {
    let mut reports: BTreeMap<&str, f64> = BTreeMap::new();
    let mut sent: BTreeMap<&str, BTreeMap<&str, f64>> = BTreeMap::new();
    let mut days = BTreeSet::new();
    for row in &data.daily {
        let day = text(row, "on_day");
        match text(row, "event") {
            "_report" => {
                days.insert(day);
                *reports.entry(day).or_default() += number(row, "count");
            }
            "question_sent" => {
                *sent
                    .entry(text(row, "bucket"))
                    .or_default()
                    .entry(day)
                    .or_default() += number(row, "count");
            }
            _ => {}
        }
    }
    let days: Vec<String> = days.into_iter().map(str::to_owned).collect();
    let series: Vec<(String, Vec<Option<f64>>)> = sent
        .iter()
        .map(|(agent, by_day)| {
            let values = reports
                .iter()
                .map(|(day, n)| by_day.get(day).map(|q| q / n))
                .collect();
            ((*agent).to_owned(), values)
        })
        .collect();
    lines(&days, &series)
}

/// The summed counts or installs of one event's buckets, in the crate's order.
fn buckets(data: &Data, event: &str, field: &str, order: &[&str]) -> Vec<(String, f64)> {
    let mut sums: BTreeMap<&str, f64> = BTreeMap::new();
    for row in data.daily.iter().filter(|row| text(row, "event") == event) {
        *sums.entry(text(row, "bucket")).or_default() += number(row, field);
    }
    order
        .iter()
        .filter_map(|name| sums.get(name).map(|n| ((*name).to_owned(), *n)))
        .collect()
}

/// One version's week in `v_tally`: its tallies and those that crashed, each `None` when hidden.
#[derive(Default)]
struct WeekTallies {
    tallies: Option<f64>,
    crashed: Option<f64>,
}

/// Crash-free weeks per version: the share of tallies with no crash, per week and version.
/// Where the crashed tallies are under 5 (hidden), the share is at least `1 - 4 / tallies`.
fn crash_free(data: &Data) -> String {
    let mut counts: BTreeMap<(&str, &str), WeekTallies> = BTreeMap::new();
    let mut weeks = BTreeSet::new();
    let mut versions = BTreeSet::new();
    for row in &data.tally {
        let key = (text(row, "version"), text(row, "week"));
        weeks.insert(text(row, "week"));
        versions.insert(text(row, "version"));
        let entry = counts.entry(key).or_default();
        match text(row, "event") {
            "_tally" => entry.tallies = Some(number(row, "count")),
            "_crashed" => entry.crashed = Some(number(row, "count")),
            _ => {}
        }
    }
    if counts.is_empty() {
        return "<p class=\"note\">No week with 5 or more tallies yet.</p>".to_owned();
    }
    let mut out = String::from("<table><tr><th>version</th>");
    for week in &weeks {
        write!(out, "<th>{}</th>", escape(week)).unwrap_or_default();
    }
    out.push_str("</tr>");
    for version in versions {
        write!(out, "<tr><th>{}</th>", escape(version)).unwrap_or_default();
        for week in &weeks {
            let cell = match counts.get(&(version, *week)) {
                Some(WeekTallies {
                    tallies: Some(tallies),
                    crashed: Some(crashed),
                }) if *tallies > 0.0 => {
                    format!("{:.0}%", 100.0 * (1.0 - crashed / tallies))
                }
                Some(WeekTallies {
                    tallies: Some(tallies),
                    crashed: None,
                }) if *tallies > 0.0 => {
                    format!("&ge;{:.0}%", (100.0 * (tallies - 4.0) / tallies).floor())
                }
                _ => String::new(),
            };
            write!(out, "<td>{cell}</td>").unwrap_or_default();
        }
        out.push_str("</tr>");
    }
    out.push_str("</table>");
    out
}

/// The cron's health: a warning for each final period with reports and no fill (lost once its
/// reports pass 90 days), and the days on which a step failed.
fn health(data: &Data) -> String {
    let mut out = String::new();
    if data.unfilled.is_empty() {
        out.push_str("<p class=\"note\">Every final week and month with reports is filled.</p>");
    } else {
        out.push_str(
            "<p class=\"alert\"><strong>Not filled</strong> (lost when their reports pass 90 \
             days):",
        );
        for row in &data.unfilled {
            write!(
                out,
                " {} {};",
                escape(text(row, "period")),
                escape(text(row, "start"))
            )
            .unwrap_or_default();
        }
        out.push_str("</p>");
    }
    let failures: Vec<&Value> = data
        .cron
        .iter()
        .filter(|row| number(row, "failed") > 0.0)
        .collect();
    let days: BTreeSet<&str> = data.cron.iter().map(|row| text(row, "day")).collect();
    write!(
        out,
        "<p class=\"note\">Cron runs recorded on {} of the last 30 days.</p>",
        days.len()
    )
    .unwrap_or_default();
    if failures.is_empty() {
        out.push_str("<p class=\"note\">No step failed in the last 30 days.</p>");
    } else {
        out.push_str("<table><tr><th>day</th><th>step</th><th>failed</th><th>ok</th></tr>");
        for row in failures {
            write!(
                out,
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(text(row, "day")),
                escape(text(row, "step")),
                short(number(row, "failed")),
                short(number(row, "ok"))
            )
            .unwrap_or_default();
        }
        out.push_str("</table>");
    }
    out
}

/// The whole page.
pub(crate) fn page(data: &Data) -> String {
    let took: Vec<&str> = Took::ALL.iter().map(|took| took.as_str()).collect();
    let steps: Vec<&str> = OnboardingStep::ALL
        .iter()
        .map(|step| step.as_str())
        .collect();
    let mut out = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"robots\" content=\"noindex\"><title>Clauseo counts</title>\
         <style>{STYLE}</style></head><body><h1>Clauseo counts</h1>\
         <p class=\"note\">From the v_* views: every group of fewer than 5 installs (or tallies) \
         is left out, so totals can run low.</p>"
    );
    let sections = [
        ("Active installs", active(data)),
        ("Weekly cohorts", cohorts(data)),
        ("Questions per active day, per agent", questions(data)),
        (
            "Answer time (last 90 days)",
            bars(&buckets(data, "answer_ended", "count", &took)),
        ),
        (
            "Onboarding funnel: installs per step (last 90 days)",
            bars(&buckets(data, "onboarding_step", "installs", &steps)),
        ),
        ("Crash-free weeks per version", crash_free(data)),
        ("The daily cron", health(data)),
    ];
    for (title, body) in sections {
        write!(out, "<h2>{title}</h2>{body}").unwrap_or_default();
    }
    out.push_str("</body></html>");
    out
}

#[cfg(test)]
mod tests;
