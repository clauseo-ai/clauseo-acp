//! `python-runtime/lock.toml`: the model, the reader and the one writer, which lays the file out
//! the same way every time so that a bump's diff shows only what changed.

use serde::Deserialize;
use std::collections::BTreeMap;

/// The lock's format; a reader refuses any other.
pub(crate) const SCHEMA: u32 = 1;

/// The whole lock.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Lock {
    pub schema: u32,
    pub runtime: Runtime,
    pub resolve: Resolve,
    /// The PBS asset of each target, keyed by triple.
    #[serde(default)]
    pub pbs: BTreeMap<String, PbsAsset>,
    #[serde(rename = "package", default)]
    pub packages: Vec<Package>,
    /// pandoc, which rides in the same archives (`pandoc.rs`).
    pub pandoc: Pandoc,
}

/// pandoc from jgm/pandoc's GitHub release: `version` is edited by hand, `bump` writes the rest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pandoc {
    /// The release tag, `3.13`.
    pub version: String,
    /// The commit the release's tag points at, as GitHub's API gives it.
    #[serde(default)]
    pub commit: String,
    /// The sha256 and size of the tag's source tarball (`pandoc::source_url`): the GPL's source
    /// pointer, and where `bump` and `build` read pandoc's licence texts.
    #[serde(default)]
    pub source_sha256: String,
    #[serde(default)]
    pub source_size: u64,
    /// The release archive of each target, keyed by triple.
    #[serde(default)]
    pub archives: BTreeMap<String, PandocArchive>,
}

/// One target's pandoc release archive.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PandocArchive {
    pub asset: String,
    /// Its sha256, which must be the `digest` GitHub's API gives the asset.
    pub sha256: String,
    pub size: u64,
    /// The path of the pandoc binary inside the archive, the one file `build` takes from it.
    pub binary: String,
}

/// What the archive is: the `CPython`, the PBS release it comes from, and Clauseo's build number.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Runtime {
    /// `3.13.18`.
    pub python: String,
    /// Clauseo's archive build number, the `c<build>` in its name and release tag. A published
    /// build is never rebuilt: any change to what goes in the archive takes a new number.
    pub build: u32,
    /// The python-build-standalone release tag, `20261026`.
    pub pbs_release: String,
}

/// What `bump` resolves from, edited by hand.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Resolve {
    /// The requirements pip resolves for each target, as pip reads them (`numpy`, `pandas<4`).
    pub requirements: Vec<String>,
    /// The SPDX licence of each package whose metadata has no `License-Expression`, keyed by
    /// normalized name.
    #[serde(default)]
    pub licences: BTreeMap<String, String>,
}

/// One target's PBS archives.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PbsAsset {
    /// The `install_only_stripped` archive the runtime is built from.
    pub asset: String,
    /// Its sha256, as PBS's `SHA256SUMS` gives it.
    pub sha256: String,
    pub size: u64,
    /// The `full` archive, which only `bump` reads, for PBS's licence files and `PYTHON.json`.
    pub full_asset: String,
    pub full_sha256: String,
}

/// One Python package and its wheel for each target it installs on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Package {
    /// The normalized name (PEP 503).
    pub name: String,
    pub version: String,
    /// SPDX expression.
    pub licence: String,
    /// The packages of the lock it depends on, on some target (normalized names).
    #[serde(default)]
    pub requires: Vec<String>,
    /// Keyed by triple.
    pub wheels: BTreeMap<String, Wheel>,
}

/// One wheel file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Wheel {
    pub file: String,
    pub url: String,
    /// From `PyPI`'s JSON `digests.sha256`, which pip's report confirms.
    pub sha256: String,
    pub size: u64,
}

impl Lock {
    /// The lock in `text`, or what `toml` says is wrong with it.
    pub(crate) fn parse(text: &str) -> Result<Lock, String> {
        toml::from_str(text).map_err(|error| error.to_string())
    }

    /// The release tag and the name of the archives' build: `python-runtime-3.13.18-c1`.
    pub(crate) fn release_tag(&self) -> String {
        format!(
            "python-runtime-{}-c{}",
            self.runtime.python, self.runtime.build
        )
    }

    /// The folder at the top of a target's archive: `python-3.13.18-c1-macos-arm64`.
    pub(crate) fn folder(&self, os_arch: (&str, &str)) -> String {
        let (os, arch) = os_arch;
        format!(
            "python-{}-c{}-{os}-{arch}",
            self.runtime.python, self.runtime.build
        )
    }

    /// A target's archive file name: `clauseo-python-3.13.18-c1-macos-arm64.tar.gz`.
    pub(crate) fn archive(&self, os_arch: (&str, &str)) -> String {
        format!("clauseo-{}.tar.gz", self.folder(os_arch))
    }

    /// The package with this normalized name.
    pub(crate) fn package(&self, name: &str) -> Option<&Package> {
        self.packages.iter().find(|package| package.name == name)
    }

    /// The lock as the one layout `bump` writes.
    pub(crate) fn render(&self) -> String {
        let mut lines: Vec<String> = HEADER.lines().map(str::to_owned).collect();
        lines.push(String::new());
        lines.push(format!("schema = {}", self.schema));
        lines.push(String::new());
        lines.push("[runtime]".to_owned());
        lines.push(format!("python = {}", quote(&self.runtime.python)));
        lines.push(format!("build = {}", self.runtime.build));
        lines.push(format!(
            "pbs_release = {}",
            quote(&self.runtime.pbs_release)
        ));
        lines.push(String::new());
        lines.extend(RESOLVE_NOTE.lines().map(str::to_owned));
        lines.push("[resolve]".to_owned());
        lines.push("requirements = [".to_owned());
        for requirement in &self.resolve.requirements {
            lines.push(format!("  {},", quote(requirement)));
        }
        lines.push("]".to_owned());
        lines.push(String::new());
        lines.push("[resolve.licences]".to_owned());
        for (name, licence) in &self.resolve.licences {
            lines.push(format!("{} = {}", key(name), quote(licence)));
        }
        for (triple, asset) in &self.pbs {
            lines.push(String::new());
            lines.push(format!("[pbs.{}]", key(triple)));
            lines.push(format!("asset = {}", quote(&asset.asset)));
            lines.push(format!("sha256 = {}", quote(&asset.sha256)));
            lines.push(format!("size = {}", asset.size));
            lines.push(format!("full_asset = {}", quote(&asset.full_asset)));
            lines.push(format!("full_sha256 = {}", quote(&asset.full_sha256)));
        }
        for package in &self.packages {
            lines.push(String::new());
            lines.push("[[package]]".to_owned());
            lines.push(format!("name = {}", quote(&package.name)));
            lines.push(format!("version = {}", quote(&package.version)));
            lines.push(format!("licence = {}", quote(&package.licence)));
            let requires: Vec<String> = package.requires.iter().map(|r| quote(r)).collect();
            lines.push(format!("requires = [{}]", requires.join(", ")));
            for (triple, wheel) in &package.wheels {
                lines.push(String::new());
                lines.push(format!("[package.wheels.{}]", key(triple)));
                lines.push(format!("file = {}", quote(&wheel.file)));
                lines.push(format!("url = {}", quote(&wheel.url)));
                lines.push(format!("sha256 = {}", quote(&wheel.sha256)));
                lines.push(format!("size = {}", wheel.size));
            }
        }
        lines.push(String::new());
        lines.extend(PANDOC_NOTE.lines().map(str::to_owned));
        lines.push("[pandoc]".to_owned());
        lines.push(format!("version = {}", quote(&self.pandoc.version)));
        lines.push(format!("commit = {}", quote(&self.pandoc.commit)));
        lines.push(format!(
            "source_sha256 = {}",
            quote(&self.pandoc.source_sha256)
        ));
        lines.push(format!("source_size = {}", self.pandoc.source_size));
        for (triple, archive) in &self.pandoc.archives {
            lines.push(String::new());
            lines.push(format!("[pandoc.archives.{}]", key(triple)));
            lines.push(format!("asset = {}", quote(&archive.asset)));
            lines.push(format!("sha256 = {}", quote(&archive.sha256)));
            lines.push(format!("size = {}", archive.size));
            lines.push(format!("binary = {}", quote(&archive.binary)));
        }
        lines.push(String::new());
        lines.join("\n")
    }
}

/// The comment above `[pandoc]`.
const PANDOC_NOTE: &str = "\
# pandoc, from jgm/pandoc's immutable GitHub release: edit only `version` by hand; bump writes the
# rest. Windows on arm64 takes the x86_64 build, which runs there under Prism.
";

/// The comment at the top of the lock.
const HEADER: &str = "\
# The inputs of Clauseo's Python runtime archives: the PBS build of CPython and every wheel, each
# pinned by sha256. `cargo xtask python-runtime bump` writes this file; edit only [runtime] and
# [resolve] by hand. `cargo xtask python-runtime check` checks it offline. See README.md.
";

/// The comment above `[resolve]`.
const RESOLVE_NOTE: &str = "\
# What bump resolves: pip resolves `requirements` for cp313 on each target (binary wheels only),
# and every package of the closure becomes a [[package]] below. `licences` gives the SPDX licence
# of a package whose metadata has no License-Expression; bump fails without one.
";

/// `text` as a TOML basic string. JSON's string escapes are TOML's, and `serde_json` never writes
/// `\\/`, the one JSON escape TOML lacks.
fn quote(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| format!("\"{text}\""))
}

/// `name` as a TOML key: bare when it is letters, digits, `-` and `_`, else quoted.
fn key(name: &str) -> String {
    if !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        name.to_owned()
    } else {
        quote(name)
    }
}
