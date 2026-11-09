//! What a wheel's file name says (PEP 427: `{dist}-{version}(-{build})?-{python}-{abi}-{platform}
//! .whl`), whether its tags fit a target, and the licence allowlist.

use super::target::Target;

/// The parts of a wheel file name. A compressed tag set (`py2.py3`) is split at the dots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WheelName<'a> {
    pub dist: &'a str,
    pub version: &'a str,
    pub python: Vec<&'a str>,
    pub abi: Vec<&'a str>,
    pub platform: Vec<&'a str>,
}

/// The parts of `file`, or why it is not a wheel's name (an sdist's `.tar.gz` or `.zip` among
/// them).
pub(crate) fn parse(file: &str) -> Result<WheelName<'_>, String> {
    let Some(stem) = file.strip_suffix(".whl") else {
        return Err(format!(
            "{file} is not a wheel (only .whl files are allowed: no sdists)"
        ));
    };
    let parts: Vec<&str> = stem.split('-').collect();
    let (dist, version, tags) = match parts.as_slice() {
        [dist, version, python, abi, platform] => (*dist, *version, [*python, *abi, *platform]),
        [dist, version, _build, python, abi, platform] => {
            (*dist, *version, [*python, *abi, *platform])
        }
        _ => return Err(format!("{file} is not a wheel name of 5 or 6 parts")),
    };
    let [python, abi, platform] = tags.map(|set| set.split('.').collect::<Vec<_>>());
    Ok(WheelName {
        dist,
        version,
        python,
        abi,
        platform,
    })
}

/// A project name as PEP 503 compares it: lowercase, each run of `-`, `_` and `.` one `-`.
pub(crate) fn normalize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut separator = false;
    for ch in name.chars() {
        if matches!(ch, '-' | '_' | '.') {
            separator = true;
        } else {
            if separator && !out.is_empty() {
                out.push('-');
            }
            separator = false;
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

/// Whether the wheel runs anywhere: every platform tag is `any`.
pub(crate) fn is_pure(wheel: &WheelName<'_>) -> bool {
    wheel.platform.iter().all(|platform| *platform == "any")
}

/// The `CPython` minor the runtime is (3.13): a `cp3N` or `py3N` tag needs `N` at most this.
const MINOR: u32 = 13;

/// Whether one python tag and one abi tag of a wheel load in `CPython` 3.13: `cp313-cp313`,
/// `cp3N-abi3` with `N` ≤ 13, or a pure `py3`/`py3N`/`cp313` with `none`.
fn pair_fits(python: &str, abi: &str) -> bool {
    let minor = |prefix: &str| {
        python
            .strip_prefix(prefix)
            .and_then(|rest| rest.parse::<u32>().ok())
    };
    match abi {
        "cp313" => python == "cp313",
        "abi3" => minor("cp3").is_some_and(|n| (2..=MINOR).contains(&n)),
        "none" => python == "py3" || python == "cp313" || minor("py3").is_some_and(|n| n <= MINOR),
        _ => false,
    }
}

/// `Ok` when `wheel` installs on `target` under `CPython` 3.13, else why not.
pub(crate) fn fits(wheel: &WheelName<'_>, target: Target) -> Result<(), String> {
    let interpreter = wheel
        .python
        .iter()
        .any(|python| wheel.abi.iter().any(|abi| pair_fits(python, abi)));
    if !interpreter {
        return Err(format!(
            "its tags {}-{} do not load in CPython 3.13 (cp313, abi3 or none)",
            wheel.python.join("."),
            wheel.abi.join(".")
        ));
    }
    if !wheel
        .platform
        .iter()
        .any(|platform| target.platform_fits(platform))
    {
        return Err(format!(
            "its platform {} does not run on {} (macOS wheels need 13.0 or older)",
            wheel.platform.join("."),
            target.triple()
        ));
    }
    Ok(())
}

/// The SPDX licence ids a bundled package may carry: permissive licences that allow
/// redistribution in a binary with a notice. No GPL, LGPL or AGPL in any version.
pub(crate) const LICENCE_ALLOWLIST: [&str; 10] = [
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "CC0-1.0",
    "ISC",
    "MIT",
    "MIT-CMU",
    "PSF-2.0",
    "Zlib",
];

/// `Ok` when every id in the SPDX expression `licence` is in `LICENCE_ALLOWLIST`, joined only by
/// `AND`, `OR` and brackets. A `WITH` exception, a `LicenseRef-` or any other word fails, so a new
/// licence is reviewed before it is added.
pub(crate) fn licence_allowed(licence: &str) -> Result<(), String> {
    let spaced = licence.replace(['(', ')'], " ");
    let mut ids = 0;
    for word in spaced.split_whitespace() {
        if matches!(word, "AND" | "OR") {
            continue;
        }
        if !LICENCE_ALLOWLIST.contains(&word) {
            return Err(format!(
                "licence `{licence}` has `{word}`, which is not in the allowlist ({})",
                LICENCE_ALLOWLIST.join(", ")
            ));
        }
        ids += 1;
    }
    if ids == 0 {
        return Err("the licence is empty".to_owned());
    }
    Ok(())
}
