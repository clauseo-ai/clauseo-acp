//! `cargo xtask python-runtime bump`: re-resolve the packages with pip for each target, take every
//! sha256 from PBS's `SHA256SUMS`, pip's report and `PyPI`'s JSON, and rewrite the lock and the
//! notices. Needs the network and a Python with pip 23.1 or newer (only to resolve: nothing is
//! installed).

use super::lock::{Lock, Package, PbsAsset, Wheel};
use super::net::{self, fetch, fetch_json, fetch_verified};
use super::target::{TARGETS, Target};
use super::{Failure, LOCK, NOTICES, assemble, rules, wheel};
use crate::output::Printer;
use crate::process::{Cmd, Program};
use crate::write;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// pip resolves the closure against `PyPI`: a minute or two.
const PIP_TIMEOUT: Duration = Duration::from_mins(10);

/// What `bump` was asked for.
pub(crate) struct Options {
    /// The Python whose pip resolves.
    pub resolver: String,
    /// Where downloads go: kept when given, else a temp folder removed at the end.
    pub cache: Option<PathBuf>,
}

/// One package pip resolved, as one target's report gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub licence_expression: Option<String>,
    pub requires_dist: Vec<String>,
}

/// Rewrites `python-runtime/lock.toml` and `THIRD-PARTY-NOTICES.txt` under `root`.
pub(crate) fn run<W: Write>(
    root: &Path,
    out: &mut Printer<W>,
    options: &Options,
) -> Result<(), Failure> {
    let lock_path = root.join(LOCK);
    let old_text = std::fs::read_to_string(&lock_path)
        .map_err(|error| Failure::io(&format!("reading {LOCK}"), &error))?;
    let old = Lock::parse(&old_text).map_err(|error| {
        Failure::new(
            format!("{LOCK} does not parse: {error}"),
            "fix [runtime] and [resolve] by hand; bump rewrites the rest",
        )
    })?;
    let (cache, temporary) = match &options.cache {
        Some(dir) => (dir.clone(), false),
        None => (
            std::env::temp_dir().join(format!("xtask-python-runtime-bump-{}", std::process::id())),
            true,
        ),
    };
    let result = bump(root, out, options, &old, &cache);
    if temporary && cache.exists() {
        let (dir, name) = super::tree::split(&cache)?;
        net::remove(&dir, &name)?;
    }
    result
}

fn bump<W: Write>(
    root: &Path,
    out: &mut Printer<W>,
    options: &Options,
    old: &Lock,
    cache: &Path,
) -> Result<(), Failure> {
    write::make_dirs(cache).map_err(|error| Failure::io("making the cache folder", &error))?;
    out.note(format!("bump: downloads go to {}", cache.display()));
    let pbs = pbs_assets(old, cache)?;
    let mut reports = BTreeMap::new();
    for target in TARGETS {
        out.note(format!("bump: resolving for {} with pip", target.triple()));
        reports.insert(target, resolve(options, old, target, cache)?);
    }
    let packages = merge(old, &reports, cache)?;
    let mut lock = Lock {
        schema: old.schema,
        runtime: old.runtime.clone(),
        resolve: old.resolve.clone(),
        pbs,
        packages,
    };
    let changed = lock.pbs != old.pbs || lock.packages != old.packages;
    if changed {
        lock.runtime.build = old.runtime.build + 1;
    }
    let found = rules::problems(&lock);
    if !found.is_empty() {
        return Err(Failure::new(
            format!(
                "the resolved lock fails its checks, so nothing was written:\n{}",
                found.join("\n")
            ),
            "change [resolve] in python-runtime/lock.toml (a requirement's bound or a licence) \
             and run bump again",
        ));
    }
    for package in &lock.packages {
        let before = old.package(&package.name).map(|p| p.version.as_str());
        if before != Some(package.version.as_str()) {
            out.note(format!(
                "bump: {} {} -> {}",
                package.name,
                before.unwrap_or("(new)"),
                package.version
            ));
        }
    }
    let text = lock.render();
    let notices = assemble::notices(options, &lock, &text, cache, out)?;
    write_repo_file(root, LOCK, &text)?;
    write_repo_file(root, NOTICES, &notices)?;
    out.note(format!(
        "bump: wrote {LOCK} (build c{}{}) and {NOTICES}",
        lock.runtime.build,
        if changed {
            ", a new build"
        } else {
            ", unchanged"
        }
    ));
    Ok(())
}

/// Writes `text` to the repo file `rel`.
pub(crate) fn write_repo_file(root: &Path, rel: &str, text: &str) -> Result<(), Failure> {
    let path = root.join(rel);
    let (dir, name) = super::tree::split(&path)?;
    write::write_file_in(&dir, &name, text.as_bytes())
        .map_err(|error| Failure::io(&format!("writing {rel}"), &error))
}

/// Each target's PBS assets, their sha256 from the release's `SHA256SUMS`, and the size of the one
/// the runtime is built from (downloaded into the cache, checked against that sha256).
fn pbs_assets(lock: &Lock, cache: &Path) -> Result<BTreeMap<String, PbsAsset>, Failure> {
    let release = &lock.runtime.pbs_release;
    let dir = cache.join("pbs");
    let sums_path = fetch(&dir, "SHA256SUMS", &net::pbs_url(release, "SHA256SUMS"))?;
    let sums = std::fs::read_to_string(&sums_path)
        .map_err(|error| Failure::io("reading PBS's SHA256SUMS", &error))?;
    let mut assets = BTreeMap::new();
    for target in TARGETS {
        let (asset, full_asset) = rules::pbs_names(lock, target);
        let sha = |name: &str| {
            net::sha256sums_entry(&sums, name).ok_or_else(|| {
                Failure::new(
                    format!("PBS release {release}'s SHA256SUMS does not list {name}"),
                    "check runtime.python and runtime.pbs_release in python-runtime/lock.toml",
                )
            })
        };
        let (sha256, full_sha256) = (sha(&asset)?, sha(&full_asset)?);
        let path = fetch_verified(&dir, &asset, &net::pbs_url(release, &asset), &sha256, None)?;
        fetch_verified(
            &dir,
            &full_asset,
            &net::pbs_url(release, &full_asset),
            &full_sha256,
            None,
        )?;
        let size = path
            .metadata()
            .map_err(|error| Failure::io(&format!("reading {asset}"), &error))?
            .len();
        assets.insert(
            target.triple().to_owned(),
            PbsAsset {
                asset,
                sha256,
                size,
                full_asset,
                full_sha256,
            },
        );
    }
    Ok(assets)
}

/// pip's resolution of `[resolve] requirements` for `target`: binary wheels only, for `CPython`
/// 3.13 on the target's platform, nothing installed.
fn resolve(
    options: &Options,
    lock: &Lock,
    target: Target,
    cache: &Path,
) -> Result<Vec<Resolved>, Failure> {
    let report = format!("report-{}.json", target.triple());
    let minor = lock
        .runtime
        .python
        .rsplit_once('.')
        .map_or("3.13", |(minor, _)| minor)
        .to_owned();
    let mut args: Vec<String> = [
        "-I",
        "-m",
        "pip",
        "install",
        "--isolated",
        "--dry-run",
        "--ignore-installed",
        "--no-cache-dir",
        "--quiet",
        "--disable-pip-version-check",
        "--report",
        &report,
        "--only-binary=:all:",
        "--platform",
        target.pip_platform(),
        "--python-version",
        &minor,
        "--implementation",
        "cp",
        "--abi",
        "cp313",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    args.extend(lock.resolve.requirements.iter().cloned());
    let cmd = Cmd::new(Program::Path(options.resolver.clone()), args, PIP_TIMEOUT);
    net::run(
        &cmd,
        cache,
        &format!("resolving for {}", target.triple()),
        "pass --resolver-python <python> with pip 23.1 or newer; when pip finds no wheel, bound \
         the requirement in [resolve] to a version that has one for every target",
    )?;
    let bytes = std::fs::read(cache.join(&report))
        .map_err(|error| Failure::io(&format!("reading pip's {report}"), &error))?;
    parse_report(&bytes)
}

/// The packages in a pip installation report (`pip install --report`).
pub(crate) fn parse_report(bytes: &[u8]) -> Result<Vec<Resolved>, Failure> {
    let bad = |what: &str| {
        Failure::new(
            format!("pip's report has no {what}"),
            "use pip 23.1 or newer, whose report has version 1",
        )
    };
    let report: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| bad(&format!("JSON ({error})")))?;
    let items = report
        .get("install")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| bad("`install` list"))?;
    let mut found = Vec::new();
    for item in items {
        let text =
            |value: Option<&serde_json::Value>| value.and_then(|v| v.as_str()).map(str::to_owned);
        let metadata = item.get("metadata").ok_or_else(|| bad("metadata"))?;
        let info = item
            .get("download_info")
            .ok_or_else(|| bad("download_info"))?;
        found.push(Resolved {
            name: wheel::normalize(&text(metadata.get("name")).ok_or_else(|| bad("name"))?),
            version: text(metadata.get("version")).ok_or_else(|| bad("version"))?,
            url: text(info.get("url")).ok_or_else(|| bad("url"))?,
            sha256: text(info.pointer("/archive_info/hashes/sha256"))
                .ok_or_else(|| bad("sha256"))?,
            licence_expression: text(metadata.get("license_expression")),
            requires_dist: metadata
                .get("requires_dist")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|r| r.as_str().map(str::to_owned))
                .collect(),
        });
    }
    Ok(found)
}

/// The names `item` depends on among `resolved`: its `Requires-Dist` entries outside any extra
/// that pip put in the same resolution (pip evaluated their markers for the target).
pub(crate) fn requires(item: &Resolved, resolved: &BTreeSet<String>) -> BTreeSet<String> {
    item.requires_dist
        .iter()
        .filter(|entry| {
            entry
                .split_once(';')
                .is_none_or(|(_, marker)| !marker.contains("extra"))
        })
        .map(|entry| rules::requirement_name(entry))
        .filter(|name| resolved.contains(name) && *name != item.name)
        .collect()
}

/// The lock's packages from each target's resolution: one version per package across targets,
/// each wheel's size from `PyPI`'s JSON (whose sha256 must be pip's), and the licence.
fn merge(
    lock: &Lock,
    reports: &BTreeMap<Target, Vec<Resolved>>,
    cache: &Path,
) -> Result<Vec<Package>, Failure> {
    let mut packages: BTreeMap<String, Package> = BTreeMap::new();
    for (target, items) in reports {
        let names: BTreeSet<String> = items.iter().map(|item| item.name.clone()).collect();
        for item in items {
            let licence = licence_of(lock, item)?;
            let package = packages
                .entry(item.name.clone())
                .or_insert_with(|| Package {
                    name: item.name.clone(),
                    version: item.version.clone(),
                    licence,
                    requires: Vec::new(),
                    wheels: BTreeMap::new(),
                });
            if package.version != item.version {
                return Err(Failure::new(
                    format!(
                        "pip resolved {} {} for {} but {} on another target",
                        item.name,
                        item.version,
                        target.triple(),
                        package.version
                    ),
                    "pin that package's version in [resolve] requirements, to one with wheels \
                     for every target",
                ));
            }
            let mut deps: BTreeSet<String> = package.requires.iter().cloned().collect();
            deps.extend(requires(item, &names));
            package.requires = deps.into_iter().collect();
            let file = item.url.rsplit('/').next().unwrap_or_default().to_owned();
            let size = pypi_size(cache, item, &file)?;
            package.wheels.insert(
                target.triple().to_owned(),
                Wheel {
                    file,
                    url: item.url.clone(),
                    sha256: item.sha256.clone(),
                    size,
                },
            );
        }
    }
    Ok(packages.into_values().collect())
}

/// The licence of `item`: its `License-Expression`, else `[resolve.licences]`. Both, when they
/// differ, or neither fail.
fn licence_of(lock: &Lock, item: &Resolved) -> Result<String, Failure> {
    match (
        &item.licence_expression,
        lock.resolve.licences.get(&item.name),
    ) {
        (Some(expression), None) => Ok(expression.clone()),
        (None, Some(given)) => Ok(given.clone()),
        (Some(expression), Some(given)) if expression == given => Err(Failure::new(
            format!(
                "{} now has License-Expression `{expression}`, the same as [resolve.licences]",
                item.name
            ),
            format!("remove `{}` from [resolve.licences]", item.name),
        )),
        (Some(expression), Some(given)) => Err(Failure::new(
            format!(
                "{} has License-Expression `{expression}`, but [resolve.licences] says `{given}`",
                item.name
            ),
            "review the package's licence and fix [resolve.licences]",
        )),
        (None, None) => Err(Failure::new(
            format!("{} {} has no License-Expression", item.name, item.version),
            format!(
                "read its licence files and add `{} = \"<SPDX>\"` to [resolve.licences]",
                item.name
            ),
        )),
    }
}

/// The size `PyPI` gives `file` of `item`, after checking `PyPI`'s sha256 is pip's and the file is
/// not yanked.
fn pypi_size(cache: &Path, item: &Resolved, file: &str) -> Result<u64, Failure> {
    let url = format!("https://pypi.org/pypi/{}/{}/json", item.name, item.version);
    let json = fetch_json(
        &cache.join("pypi"),
        &format!("{}-{}.json", item.name, item.version),
        &url,
    )?;
    let entry = json
        .get("urls")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|entry| entry.get("filename").and_then(|f| f.as_str()) == Some(file))
        .ok_or_else(|| Failure::new(format!("{url} does not list {file}"), "try again later"))?;
    let sha = entry.pointer("/digests/sha256").and_then(|s| s.as_str());
    if sha != Some(item.sha256.as_str()) {
        return Err(Failure::new(
            format!(
                "PyPI gives {file} sha256 {sha:?}, but pip's report says {}",
                item.sha256
            ),
            "do not use this file; try again later and report it",
        ));
    }
    if entry.get("yanked").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err(Failure::new(
            format!("{file} is yanked on PyPI"),
            "bound the requirement in [resolve] to another version",
        ));
    }
    entry
        .get("size")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| Failure::new(format!("{url} gives {file} no size"), "try again later"))
}
