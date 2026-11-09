//! `cargo xtask python-runtime check --online`: the nightly re-check. Every pinned wheel must still
//! be on `PyPI` with the lock's sha256 and size and not yanked, every PBS asset must still have the
//! lock's sha256 in its release's `SHA256SUMS`, and OSV is asked for advisories against each
//! pinned package version. Advisories are reported (and written as JSON for the workflow's
//! issue job); they do not fail the check, a changed hash does.

use super::Failure;
use super::lock::Lock;
use super::net::{self, fetch, fetch_json};
use crate::output::Printer;
use crate::process::{Cmd, Program};
use crate::write;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// OSV's batch query answers in seconds.
const OSV_TIMEOUT: Duration = Duration::from_mins(5);
const OSV_URL: &str = "https://api.osv.dev/v1/querybatch";

/// One advisory OSV lists for a pinned package version.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Advisory {
    pub package: String,
    pub version: String,
    pub id: String,
}

/// Runs the online re-check in the scratch folder `work`, and writes the advisories as a JSON
/// list to `advisories` when given.
pub(crate) fn run<W: Write>(
    lock: &Lock,
    work: &Path,
    advisories: Option<&Path>,
    out: &mut Printer<W>,
) -> Result<(), Failure> {
    let mut problems = Vec::new();
    for package in &lock.packages {
        let url = format!(
            "https://pypi.org/pypi/{}/{}/json",
            package.name, package.version
        );
        let json = fetch_json(&work.join("pypi"), &format!("{}.json", package.name), &url)?;
        let urls = json.get("urls").and_then(serde_json::Value::as_array);
        for (triple, wheel) in &package.wheels {
            let entry = urls
                .into_iter()
                .flatten()
                .find(|entry| entry.get("filename").and_then(|f| f.as_str()) == Some(&wheel.file));
            let Some(entry) = entry else {
                problems.push(format!("{triple}: PyPI no longer lists {}", wheel.file));
                continue;
            };
            let sha = entry.pointer("/digests/sha256").and_then(|s| s.as_str());
            let size = entry.get("size").and_then(serde_json::Value::as_u64);
            let yanked = entry.get("yanked").and_then(serde_json::Value::as_bool) == Some(true);
            if sha != Some(wheel.sha256.as_str()) || size != Some(wheel.size) {
                problems.push(format!(
                    "{triple}: PyPI gives {} sha256 {sha:?} and size {size:?}, but the lock pins \
                     {} and {}",
                    wheel.file, wheel.sha256, wheel.size
                ));
            }
            if yanked {
                problems.push(format!("{triple}: {} is yanked on PyPI", wheel.file));
            }
        }
    }
    let release = &lock.runtime.pbs_release;
    let sums_path = fetch(work, "SHA256SUMS", &net::pbs_url(release, "SHA256SUMS"))?;
    let sums = std::fs::read_to_string(&sums_path)
        .map_err(|error| Failure::io("reading PBS's SHA256SUMS", &error))?;
    for (triple, asset) in &lock.pbs {
        for (name, sha) in [
            (&asset.asset, &asset.sha256),
            (&asset.full_asset, &asset.full_sha256),
        ] {
            if net::sha256sums_entry(&sums, name).as_deref() != Some(sha.as_str()) {
                problems.push(format!(
                    "{triple}: PBS's SHA256SUMS no longer gives {name} {sha}"
                ));
            }
        }
    }
    let found = osv(lock, work)?;
    for advisory in &found {
        out.note(format!(
            "advisory: {} {} is affected by {} (https://osv.dev/vulnerability/{})",
            advisory.package, advisory.version, advisory.id, advisory.id
        ));
    }
    if found.is_empty() {
        out.note("check --online: OSV lists no advisory for any pinned package version");
    }
    if let Some(path) = advisories {
        let json = serde_json::to_string_pretty(&found).unwrap_or_else(|_| "[]".to_owned());
        let (dir, name) = super::tree::split(path)?;
        write::write_file_in(&dir, &name, json.as_bytes())
            .map_err(|error| Failure::io("writing the advisories", &error))?;
    }
    if problems.is_empty() {
        out.note(format!(
            "check --online: {} packages and {} PBS assets still match the lock",
            lock.packages.len(),
            lock.pbs.len()
        ));
        Ok(())
    } else {
        Err(Failure::new(
            format!(
                "the lock no longer matches upstream:\n{}",
                problems.join("\n")
            ),
            "do not build until this is understood: a changed hash means the file changed. \
             Review, then run `cargo xtask python-runtime bump`",
        ))
    }
}

/// OSV's advisories for every pinned package version, in the lock's order.
fn osv(lock: &Lock, work: &Path) -> Result<Vec<Advisory>, Failure> {
    let queries: Vec<serde_json::Value> = lock
        .packages
        .iter()
        .map(|package| {
            serde_json::json!({
                "package": { "name": package.name, "ecosystem": "PyPI" },
                "version": package.version,
            })
        })
        .collect();
    let body = serde_json::json!({ "queries": queries }).to_string();
    write::write_file_in(work, "osv-query.json", body.as_bytes())
        .map_err(|error| Failure::io("writing the OSV query", &error))?;
    let cmd = Cmd::new(
        Program::Named("curl"),
        [
            "-sSfL",
            "--proto",
            "=https",
            "--retry",
            "3",
            "--retry-all-errors",
            "--max-time",
            "120",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@osv-query.json",
            "-o",
            "osv-result.json",
            OSV_URL,
        ],
        OSV_TIMEOUT,
    );
    net::run(
        &cmd,
        work,
        "asking OSV for advisories",
        "check the network and try again",
    )?;
    let bytes = std::fs::read(work.join("osv-result.json"))
        .map_err(|error| Failure::io("reading OSV's answer", &error))?;
    parse_osv(lock, &bytes)
}

/// The advisories in OSV's batch answer, whose `results` follow the query's order.
pub(crate) fn parse_osv(lock: &Lock, bytes: &[u8]) -> Result<Vec<Advisory>, Failure> {
    let answer: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
        Failure::new(
            format!("OSV's answer is not JSON: {error}"),
            "try again later",
        )
    })?;
    let results = answer
        .get("results")
        .and_then(serde_json::Value::as_array)
        .filter(|results| results.len() == lock.packages.len())
        .ok_or_else(|| {
            Failure::new(
                "OSV's answer has no result for each package".to_owned(),
                "try again later",
            )
        })?;
    let mut found = Vec::new();
    for (package, result) in lock.packages.iter().zip(results) {
        for vuln in result
            .get("vulns")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = vuln.get("id").and_then(|id| id.as_str()) {
                found.push(Advisory {
                    package: package.name.clone(),
                    version: package.version.clone(),
                    id: id.to_owned(),
                });
            }
        }
    }
    Ok(found)
}
