//! The offline checks of the lock: what `cargo xtask python-runtime check` runs, and what the
//! xtask test over the real `python-runtime/lock.toml` runs inside `cargo xtask check`.

use super::lock::{Lock, Package, SCHEMA};
use super::target::{TARGETS, Target};
use super::wheel::{self, WheelName};
use std::collections::{BTreeSet, VecDeque};

/// Every problem with `lock`, each as one sentence. Empty when it passes.
pub(crate) fn problems(lock: &Lock) -> Vec<String> {
    let mut found = Vec::new();
    if lock.schema != SCHEMA {
        found.push(format!(
            "schema is {}, but this xtask reads {SCHEMA}",
            lock.schema
        ));
    }
    runtime(lock, &mut found);
    pbs(lock, &mut found);
    packages(lock, &mut found);
    closure(lock, &mut found);
    found
}

/// The `CPython` must be a 3.13 (the tag rules assume it) and the build number positive.
fn runtime(lock: &Lock, found: &mut Vec<String>) {
    let python = &lock.runtime.python;
    let patch = python.strip_prefix("3.13.");
    if !patch.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
        found.push(format!(
            "runtime.python is `{python}`, but the wheel tag rules are for CPython 3.13.x"
        ));
    }
    if lock.runtime.build == 0 {
        found.push("runtime.build is 0; builds count from 1".to_owned());
    }
    let release = &lock.runtime.pbs_release;
    if release.len() != 8 || !release.bytes().all(|b| b.is_ascii_digit()) {
        found.push(format!(
            "runtime.pbs_release is `{release}`, not a PBS release tag (YYYYMMDD)"
        ));
    }
}

/// The PBS asset names a target's archives must have.
pub(crate) fn pbs_names(lock: &Lock, target: Target) -> (String, String) {
    let stem = format!(
        "cpython-{}+{}-{}",
        lock.runtime.python,
        lock.runtime.pbs_release,
        target.triple()
    );
    let full = if target.is_macos() { "pgo+lto" } else { "pgo" };
    (
        format!("{stem}-install_only_stripped.tar.gz"),
        format!("{stem}-{full}-full.tar.zst"),
    )
}

fn pbs(lock: &Lock, found: &mut Vec<String>) {
    for triple in lock.pbs.keys() {
        if Target::from_triple(triple).is_none() {
            found.push(format!("[pbs.{triple}] is not one of the four targets"));
        }
    }
    for target in TARGETS {
        let triple = target.triple();
        let Some(asset) = lock.pbs.get(triple) else {
            found.push(format!("[pbs.{triple}] is missing"));
            continue;
        };
        let (install, full) = pbs_names(lock, target);
        if asset.asset != install {
            found.push(format!(
                "[pbs.{triple}] asset is `{}`, but runtime pins `{install}`",
                asset.asset
            ));
        }
        if asset.full_asset != full {
            found.push(format!(
                "[pbs.{triple}] full_asset is `{}`, but runtime pins `{full}`",
                asset.full_asset
            ));
        }
        for (field, sha) in [
            ("sha256", &asset.sha256),
            ("full_sha256", &asset.full_sha256),
        ] {
            if !is_sha256(sha) {
                found.push(format!(
                    "[pbs.{triple}] {field} is not 64 lowercase hex digits"
                ));
            }
        }
        if asset.size == 0 {
            found.push(format!("[pbs.{triple}] size is 0"));
        }
    }
}

/// Whether `text` is a sha256 in lowercase hex.
pub(crate) fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn packages(lock: &Lock, found: &mut Vec<String>) {
    let names: Vec<&str> = lock.packages.iter().map(|p| p.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted != names {
        found.push("the [[package]] entries are not sorted by name, or a name repeats".to_owned());
    }
    if lock.packages.is_empty() {
        found.push("the lock has no [[package]]".to_owned());
    }
    for package in &lock.packages {
        one_package(package, found);
    }
    for (name, licence) in &lock.resolve.licences {
        match lock.package(name) {
            None => found.push(format!(
                "resolve.licences names `{name}`, which is not a package of the lock"
            )),
            Some(package) if package.licence != *licence => found.push(format!(
                "resolve.licences gives `{name}` `{licence}`, but its [[package]] says `{}`",
                package.licence
            )),
            Some(_) => {}
        }
    }
}

fn one_package(package: &Package, found: &mut Vec<String>) {
    let name = &package.name;
    if wheel::normalize(name) != *name {
        found.push(format!("package `{name}` is not a normalized name"));
    }
    if package.version.is_empty() {
        found.push(format!("package `{name}` has no version"));
    }
    if let Err(why) = wheel::licence_allowed(&package.licence) {
        found.push(format!("package `{name}`: {why}"));
    }
    if package.wheels.is_empty() {
        found.push(format!("package `{name}` has no wheel"));
    }
    let mut native = false;
    let mut pure_files = BTreeSet::new();
    for (triple, file) in &package.wheels {
        let Some(target) = Target::from_triple(triple) else {
            found.push(format!(
                "package `{name}` has a wheel for `{triple}`, not one of the four targets"
            ));
            continue;
        };
        let here = format!("package `{name}`, {triple}");
        if !is_sha256(&file.sha256) {
            found.push(format!("{here}: sha256 is not 64 lowercase hex digits"));
        }
        if file.size == 0 {
            found.push(format!("{here}: size is 0"));
        }
        let url_ok = file.url.starts_with("https://files.pythonhosted.org/")
            && file.url.ends_with(&format!("/{}", file.file));
        if !url_ok {
            found.push(format!(
                "{here}: url is not files.pythonhosted.org's for {}",
                file.file
            ));
        }
        match wheel::parse(&file.file) {
            Err(why) => found.push(format!("{here}: {why}")),
            Ok(parsed) => {
                tags(&here, package, &parsed, target, found);
                if wheel::is_pure(&parsed) {
                    pure_files.insert((file.file.as_str(), file.sha256.as_str()));
                } else {
                    native = true;
                }
            }
        }
    }
    if native {
        for target in TARGETS {
            if !package.wheels.contains_key(target.triple()) {
                found.push(format!(
                    "package `{name}` has compiled code but no wheel for {}",
                    target.triple()
                ));
            }
        }
    }
    if pure_files.len() > 1 {
        found.push(format!(
            "package `{name}` has different pure-Python wheels on different targets"
        ));
    }
}

/// The wheel's name, version and tags against the package and target.
fn tags(
    here: &str,
    package: &Package,
    parsed: &WheelName<'_>,
    target: Target,
    found: &mut Vec<String>,
) {
    if wheel::normalize(parsed.dist) != package.name {
        found.push(format!(
            "{here}: the wheel is of `{}`, not `{}`",
            parsed.dist, package.name
        ));
    }
    if parsed.version != package.version {
        found.push(format!(
            "{here}: the wheel is version {}, not {}",
            parsed.version, package.version
        ));
    }
    if let Err(why) = wheel::fits(parsed, target) {
        found.push(format!("{here}: {why}"));
    }
}

/// The name a requirement names: `pandas` of `pandas<4; python_version >= "3.13"`.
pub(crate) fn requirement_name(requirement: &str) -> String {
    let end = requirement
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
        .unwrap_or(requirement.len());
    wheel::normalize(requirement.get(..end).unwrap_or(requirement))
}

/// Every requirement and every `requires` is a package of the lock, and every package is reached
/// from the requirements: the lock is the closure, no more and no less.
fn closure(lock: &Lock, found: &mut Vec<String>) {
    let mut queue = VecDeque::new();
    for requirement in &lock.resolve.requirements {
        let name = requirement_name(requirement);
        if lock.package(&name).is_none() {
            found.push(format!(
                "requirement `{requirement}` has no [[package]] `{name}`"
            ));
        } else {
            queue.push_back(name);
        }
    }
    if lock.resolve.requirements.is_empty() {
        found.push("resolve.requirements is empty".to_owned());
    }
    for package in &lock.packages {
        for dependency in &package.requires {
            if dependency == &package.name || lock.package(dependency).is_none() {
                found.push(format!(
                    "package `{}` requires `{dependency}`, which is not another package of the \
                     lock",
                    package.name
                ));
            }
        }
    }
    let mut reached = BTreeSet::new();
    while let Some(name) = queue.pop_front() {
        if !reached.insert(name.clone()) {
            continue;
        }
        if let Some(package) = lock.package(&name) {
            queue.extend(package.requires.iter().cloned());
        }
    }
    for package in &lock.packages {
        if !reached.contains(&package.name) {
            found.push(format!(
                "package `{}` is not required by the requirements or any package they need",
                package.name
            ));
        }
    }
}
