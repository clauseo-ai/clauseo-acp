//! pandoc in the runtime: the `pandoc` binary from jgm/pandoc's GitHub release archive of each
//! target, pinned by sha256 in `[pandoc]` of the lock, unpacked into the runtime as `bin/pandoc`
//! (macOS) or `clauseo-bin\pandoc.exe` (Windows). Windows on arm64 takes the `x86_64` build, which
//! runs there under Prism. Nothing else from the archive goes in: the licence texts are in the
//! notices, and nothing outside the agent's environment puts the folder on PATH.
//!
//! What is checked, and why it is not PBS's check. jgm/pandoc publishes no build provenance and no
//! signature files: `gh attestation verify pandoc-3.13-windows-x86_64.zip --repo jgm/pandoc`
//! answers `HTTP 404` (no attestation; gh 2.103.3, 2026-11-09), and the release has no `.asc`,
//! `.sig` or checksum asset. Its releases are GitHub immutable releases, though, which carry
//! GitHub's signed release attestation of the tag and every asset's digest:
//! `gh release verify-asset 3.13 pandoc-3.13-windows-x86_64.zip -R jgm/pandoc` exits 0, and 1 for
//! a copy with one byte added. So:
//! - `bump` takes each asset's sha256 from GitHub's API (`digest`), refuses a release that is not
//!   immutable, and downloads every archive against that digest;
//! - `build` downloads over HTTPS from the release, checks the lock's sha256 and size, then runs
//!   `gh release verify-asset` (fails closed, like PBS's attestation);
//! - `check --online` re-checks the digests, the sizes and the source tarball nightly.
//!
//! pandoc is GPL-2.0-or-later. It runs as its own program, never loaded into Python, so the
//! packages' licence allowlist (`rules.rs`) does not apply to it; the notices carry its
//! `COPYING.md` and `COPYRIGHT` word for word and point at the exact source tarball of the pinned
//! tag.

use super::Failure;
use super::lock::{Lock, Pandoc, PandocArchive};
use super::net::{self, fetch, fetch_json, fetch_verified};
use super::notices::Section;
use super::rules::is_sha256;
use super::target::{TARGETS, Target};
use crate::process::{Cmd, Program};
use crate::write;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The repository whose releases pandoc comes from.
pub(crate) const REPO: &str = "jgm/pandoc";
/// pandoc's licence (its `COPYRIGHT`: "version 2 or later").
pub(crate) const LICENCE: &str = "GPL-2.0-or-later";
/// The licence files of the source tarball the notices carry, under its `pandoc-<version>/`.
pub(crate) const SOURCE_TEXTS: [&str; 2] = ["COPYING.md", "COPYRIGHT"];
/// The plain-text licence files of the Windows archive, beside `pandoc.exe`; `build` checks they
/// are in the notices too. (Its `COPYING.rtf` is the GPL of `COPYING.md` as RTF.)
pub(crate) const WINDOWS_TEXTS: [&str; 1] = ["COPYRIGHT.txt"];
/// `gh release verify-asset` fetches the release attestation and the trust root: seconds.
const GH_TIMEOUT: Duration = Duration::from_mins(5);
/// Python copies a file of a few hundred MB out of an archive: seconds.
const EXTRACT_TIMEOUT: Duration = Duration::from_mins(5);

/// Lists the members of the zip in `argv[1]`, one per line.
const LIST_ZIP: &str =
    "import sys, zipfile\nprint('\\n'.join(zipfile.ZipFile(sys.argv[1]).namelist()))\n";

/// Copies members out of the zip or tar.gz in `argv[1]`: then `<member> <dest> <octal mode>`
/// triples. A member that is not a regular file, or a dest that exists, fails.
const EXTRACT: &str = "import os, shutil, sys, tarfile, zipfile\n\
    archive, rest = sys.argv[1], sys.argv[2:]\n\
    if archive.endswith('.zip'):\n\
    \x20   z = zipfile.ZipFile(archive)\n\
    \x20   open_member = z.open\n\
    else:\n\
    \x20   t = tarfile.open(archive, 'r:gz')\n\
    \x20   def open_member(name):\n\
    \x20       f = t.extractfile(t.getmember(name))\n\
    \x20       if f is None:\n\
    \x20           raise SystemExit(name + ' is not a regular file')\n\
    \x20       return f\n\
    for i in range(0, len(rest), 3):\n\
    \x20   member, dest, mode = rest[i:i + 3]\n\
    \x20   with open_member(member) as src, open(dest, 'xb') as out:\n\
    \x20       shutil.copyfileobj(src, out, 1 << 20)\n\
    \x20   os.chmod(dest, int(mode, 8))\n";

/// A target's release asset: `pandoc-3.13-arm64-macOS.zip`.
pub(crate) fn asset_name(version: &str, target: Target) -> String {
    match target {
        Target::MacArm64 => format!("pandoc-{version}-arm64-macOS.zip"),
        Target::MacX64 => format!("pandoc-{version}-x86_64-macOS.zip"),
        Target::WinArm64 | Target::WinX64 => format!("pandoc-{version}-windows-x86_64.zip"),
    }
}

/// The download URL of a release asset.
pub(crate) fn asset_url(version: &str, asset: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{version}/{asset}")
}

/// The source tarball of the release's tag: the source pointer the notices give.
pub(crate) fn source_url(version: &str) -> String {
    format!("https://github.com/{REPO}/archive/refs/tags/{version}.tar.gz")
}

/// The source tarball's file name in a download folder.
fn source_file(version: &str) -> String {
    format!("pandoc-{version}-source.tar.gz")
}

/// Where the binary goes, relative to the runtime's root.
pub(crate) fn dest(target: Target) -> &'static str {
    if target.is_macos() {
        "bin/pandoc"
    } else {
        "clauseo-bin/pandoc.exe"
    }
}

/// The end of the binary's path inside a target's archive.
fn binary_suffix(target: Target) -> &'static str {
    if target.is_macos() {
        "/bin/pandoc"
    } else {
        "/pandoc.exe"
    }
}

/// The section title in the notices: `pandoc 3.13`.
pub(crate) fn title(pandoc: &Pandoc) -> String {
    format!("pandoc {}", pandoc.version)
}

/// Every problem with `[pandoc]`, added to `found`.
pub(crate) fn problems(lock: &Lock, found: &mut Vec<String>) {
    let pandoc = &lock.pandoc;
    let version = &pandoc.version;
    let parts_ok = version
        .split('.')
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if !parts_ok {
        found.push(format!(
            "pandoc.version is `{version}`, not a pandoc release (3.13)"
        ));
    }
    let hex40 = pandoc.commit.len() == 40
        && pandoc
            .commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !hex40 {
        found.push("pandoc.commit is not a 40-digit lowercase commit id".to_owned());
    }
    if !is_sha256(&pandoc.source_sha256) || pandoc.source_size == 0 {
        found.push("pandoc.source_sha256 or source_size is missing".to_owned());
    }
    for triple in pandoc.archives.keys() {
        if Target::from_triple(triple).is_none() {
            found.push(format!(
                "[pandoc.archives.{triple}] is not one of the four targets"
            ));
        }
    }
    for target in TARGETS {
        let triple = target.triple();
        let Some(archive) = pandoc.archives.get(triple) else {
            found.push(format!("[pandoc.archives.{triple}] is missing"));
            continue;
        };
        let want = asset_name(version, target);
        if archive.asset != want {
            found.push(format!(
                "[pandoc.archives.{triple}] asset is `{}`, but pandoc.version pins `{want}`",
                archive.asset
            ));
        }
        if !is_sha256(&archive.sha256) || archive.size == 0 {
            found.push(format!(
                "[pandoc.archives.{triple}] sha256 or size is missing"
            ));
        }
        let binary = &archive.binary;
        if !binary.ends_with(binary_suffix(target))
            || binary.starts_with('/')
            || binary
                .split('/')
                .any(|part| part == ".." || part.is_empty())
        {
            found.push(format!(
                "[pandoc.archives.{triple}] binary `{binary}` is not a `…{}` inside the archive",
                binary_suffix(target)
            ));
        }
    }
    if pandoc.archives.get(Target::WinArm64.triple())
        != pandoc.archives.get(Target::WinX64.triple())
    {
        found.push(
            "[pandoc.archives] Windows arm64 must take the x86_64 archive (pandoc ships no arm64 \
             build; Prism runs it)"
                .to_owned(),
        );
    }
}

/// The release's JSON from GitHub's API, refused unless it is a published, immutable release.
fn release_json(version: &str, dir: &Path) -> Result<serde_json::Value, Failure> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/tags/{version}");
    let json = fetch_json(dir, &format!("release-{version}.json"), &url)?;
    let flag = |name: &str| json.get(name).and_then(serde_json::Value::as_bool);
    if flag("draft") != Some(false) || flag("prerelease") != Some(false) {
        return Err(Failure::new(
            format!("pandoc {version} is a draft or a pre-release on GitHub"),
            "set pandoc.version to a stable release (`gh release view -R jgm/pandoc`)",
        ));
    }
    if flag("immutable") != Some(true) {
        return Err(Failure::new(
            format!("pandoc {version} is not an immutable release, so gh cannot verify its assets"),
            "use an immutable pandoc release, or review how its archives are checked first",
        ));
    }
    Ok(json)
}

/// The sha256 (from `digest`) and size GitHub's API gives `asset` of `release`.
fn asset_digest(release: &serde_json::Value, asset: &str) -> Option<(String, u64)> {
    let entry = release
        .get("assets")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .find(|entry| entry.get("name").and_then(|n| n.as_str()) == Some(asset))?;
    let sha = entry.get("digest")?.as_str()?.strip_prefix("sha256:")?;
    let size = entry.get("size")?.as_u64()?;
    Some((sha.to_owned(), size))
}

/// `bump`'s `[pandoc]`: `old`'s version, with the commit, source tarball and every archive taken
/// afresh, each archive downloaded into `<cache>/pandoc` against GitHub's digest.
pub(crate) fn bump(old: &Pandoc, python: &str, cache: &Path) -> Result<Pandoc, Failure> {
    let version = &old.version;
    let dir = cache.join("pandoc");
    let release = release_json(version, &dir)?;
    let commit_url = format!("https://api.github.com/repos/{REPO}/commits/{version}");
    let commit = fetch_json(&dir, &format!("commit-{version}.json"), &commit_url)?
        .get("sha")
        .and_then(|sha| sha.as_str())
        .map(str::to_owned)
        .ok_or_else(|| Failure::new(format!("{commit_url} gives no sha"), "try again later"))?;
    let mut archives = BTreeMap::new();
    for target in TARGETS {
        let asset = asset_name(version, target);
        let (sha256, size) = asset_digest(&release, &asset).ok_or_else(|| {
            Failure::new(
                format!("pandoc {version}'s release has no asset {asset} with a digest"),
                "check pandoc.version; a release without that archive cannot go in the runtime",
            )
        })?;
        let path = fetch_verified(
            &dir,
            &asset,
            &asset_url(version, &asset),
            &sha256,
            Some(size),
        )?;
        let listing = run_python(python, LIST_ZIP, &[path.to_string_lossy().as_ref()], &dir)?;
        let binaries: Vec<&str> = listing
            .lines()
            .filter(|line| line.ends_with(binary_suffix(target)))
            .collect();
        let [binary] = binaries.as_slice() else {
            return Err(Failure::new(
                format!(
                    "{asset} holds {} files ending in {}, not one",
                    binaries.len(),
                    binary_suffix(target)
                ),
                "check the archive's layout",
            ));
        };
        archives.insert(
            target.triple().to_owned(),
            PandocArchive {
                asset,
                sha256,
                size,
                binary: (*binary).to_owned(),
            },
        );
    }
    let source = dir.join(source_file(version));
    if source.exists() {
        net::remove(&dir, &source_file(version))?;
    }
    fetch(&dir, &source_file(version), &source_url(version))?;
    let source_sha256 = net::sha256_file(&source)
        .map_err(|error| Failure::io("reading pandoc's source tarball", &error))?;
    let source_size = source
        .metadata()
        .map_err(|error| Failure::io("reading pandoc's source tarball", &error))?
        .len();
    Ok(Pandoc {
        version: version.clone(),
        commit,
        source_sha256,
        source_size,
        archives,
    })
}

/// Runs `script` with `python` (isolated) and `args` in `dir`; returns what it printed.
fn run_python(python: &str, script: &str, args: &[&str], dir: &Path) -> Result<String, Failure> {
    let mut all = vec!["-I", "-B", "-c", script];
    all.extend_from_slice(args);
    let cmd = Cmd::new(Program::Path(python.to_owned()), all, EXTRACT_TIMEOUT);
    net::run(
        &cmd,
        dir,
        "reading a pandoc archive",
        "check the archive and the Python that reads it",
    )
}

/// Copies `member` of `archive` to `dest` (which must not exist) with `mode`, using `python`.
fn extract(
    python: &str,
    archive: &Path,
    pairs: &[(&str, &Path, &str)],
    dir: &Path,
) -> Result<(), Failure> {
    let mut args = vec![archive.to_string_lossy().into_owned()];
    for (member, dest, mode) in pairs {
        args.push((*member).to_owned());
        args.push(dest.to_string_lossy().into_owned());
        args.push((*mode).to_owned());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_python(python, EXTRACT, &refs, dir).map(drop)
}

/// The source tarball, downloaded into `dir` against the lock's sha256 and size.
fn fetch_source(pandoc: &Pandoc, dir: &Path) -> Result<PathBuf, Failure> {
    fetch_verified(
        dir,
        &source_file(&pandoc.version),
        &source_url(&pandoc.version),
        &pandoc.source_sha256,
        Some(pandoc.source_size),
    )
}

/// A target's archive, downloaded into `dir` against the lock's sha256 and size.
fn fetch_archive(pandoc: &Pandoc, target: Target, dir: &Path) -> Result<PathBuf, Failure> {
    let archive = archive_of(pandoc, target)?;
    fetch_verified(
        dir,
        &archive.asset,
        &asset_url(&pandoc.version, &archive.asset),
        &archive.sha256,
        Some(archive.size),
    )
}

fn archive_of(pandoc: &Pandoc, target: Target) -> Result<&PandocArchive, Failure> {
    pandoc.archives.get(target.triple()).ok_or_else(|| {
        Failure::new(
            format!("the lock has no [pandoc.archives.{}]", target.triple()),
            "run `cargo xtask python-runtime check`",
        )
    })
}

/// The licence texts of pandoc, read with `python` from the source tarball and each target's
/// archive in `dir` (each checked against the lock first): `(path, body, target)`, the source's
/// files once per target. Unpacks into `<dir>/pandoc-licences`, made afresh.
pub(crate) fn licence_texts(
    pandoc: &Pandoc,
    targets: &[Target],
    python: &str,
    dir: &Path,
) -> Result<Vec<(String, String, Target)>, Failure> {
    let out = dir.join("pandoc-licences");
    if out.exists() {
        net::remove(dir, "pandoc-licences")?;
    }
    write::make_dirs(&out).map_err(|error| Failure::io("making an unpack folder", &error))?;
    let source = fetch_source(pandoc, dir)?;
    let top = format!("pandoc-{}", pandoc.version);
    let members: Vec<(String, PathBuf)> = SOURCE_TEXTS
        .iter()
        .map(|name| (format!("{top}/{name}"), out.join(name)))
        .collect();
    let pairs: Vec<(&str, &Path, &str)> = members
        .iter()
        .map(|(member, dest)| (member.as_str(), dest.as_path(), "644"))
        .collect();
    extract(python, &source, &pairs, dir)?;
    let mut found = Vec::new();
    for (member, dest) in &members {
        let body = read_text(dest)?;
        found.extend(targets.iter().map(|t| (member.clone(), body.clone(), *t)));
    }
    for target in targets.iter().filter(|t| !t.is_macos()) {
        let archive = archive_of(pandoc, *target)?;
        let path = fetch_archive(pandoc, *target, dir)?;
        let folder = archive
            .binary
            .rsplit_once('/')
            .map_or("", |(folder, _)| folder);
        for name in WINDOWS_TEXTS {
            let member = format!("{folder}/{name}");
            let dest = out.join(format!("{}-{name}", target.triple()));
            extract(python, &path, &[(&member, &dest, "644")], dir)?;
            found.push((member, read_text(&dest)?, *target));
        }
    }
    Ok(found)
}

/// `build`'s step 1 for pandoc: the archive and the source tarball into `dir`, each against the
/// lock's sha256 and size, then (unless `no_attestation`) GitHub's release attestation of the
/// archive with `gh release verify-asset`, failing closed. Returns the archive's path.
pub(crate) fn inputs(
    pandoc: &Pandoc,
    target: Target,
    dir: &Path,
    no_attestation: bool,
) -> Result<PathBuf, Failure> {
    let path = fetch_archive(pandoc, target, dir)?;
    fetch_source(pandoc, dir)?;
    if !no_attestation {
        verify_release_asset(&pandoc.version, &path)?;
    }
    Ok(path)
}

/// Checks with `gh release verify-asset` that `file` is an asset of jgm/pandoc's release
/// `version`, by GitHub's signed release attestation. Any error from gh fails.
pub(crate) fn verify_release_asset(version: &str, file: &Path) -> Result<(), Failure> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cmd = Cmd::new(
        Program::Named("gh"),
        ["release", "verify-asset", version, &name, "--repo", REPO],
        GH_TIMEOUT,
    );
    net::run(
        &cmd,
        dir,
        &format!("checking GitHub's release attestation of {name}"),
        "do not build from this file. gh 2.75 or newer must be on PATH with a token (GH_TOKEN); \
         `--no-attestation` skips this for a local try only, never in CI",
    )
    .map(drop)
}

/// `build`'s pandoc step: copies only the binary out of `archive` into `runtime` at `dest`, with
/// `python` (the runtime's own). Fails when something is already there.
pub(crate) fn install(
    pandoc: &Pandoc,
    target: Target,
    archive: &Path,
    runtime: &Path,
    python: &Path,
) -> Result<PathBuf, Failure> {
    let binary = &archive_of(pandoc, target)?.binary;
    let dest = runtime.join(dest(target));
    if dest.exists() {
        return Err(Failure::new(
            format!("{} already exists in the runtime", dest.display()),
            "find what put it there; pandoc's place must be free",
        ));
    }
    if let Some(parent) = dest.parent() {
        write::make_dirs(parent).map_err(|error| Failure::io("making pandoc's folder", &error))?;
    }
    let dir = archive.parent().unwrap_or(Path::new("."));
    extract(
        &python.to_string_lossy(),
        archive,
        &[(binary, &dest, "755")],
        dir,
    )?;
    Ok(dest)
}

/// The notices section of pandoc, from its licence texts.
pub(crate) fn section(pandoc: &Pandoc, texts: &[(String, String, Target)]) -> Section {
    let mut section = Section::new(
        title(pandoc),
        LICENCE.to_owned(),
        format!(
            "The pandoc program, unchanged, from jgm/pandoc's release {} (commit {}): bin/pandoc \
             on macOS,\nclauseo-bin\\pandoc.exe on Windows (the x86_64 build on both Windows \
             targets).\nIts complete corresponding source code: {}\n(sha256 {}), also at \
             https://github.com/{REPO}/tree/{}.",
            pandoc.version,
            pandoc.commit,
            source_url(&pandoc.version),
            pandoc.source_sha256,
            pandoc.commit
        ),
    );
    for (path, body, target) in texts {
        if !section
            .texts
            .iter()
            .any(|text| text.body == super::notices::normalize(body) && text.path != *path)
        {
            section.add(path, body, target.triple());
        }
    }
    section
}

fn read_text(path: &Path) -> Result<String, Failure> {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|error| Failure::io(&format!("reading {}", path.display()), &error))
}

/// `check --online` for pandoc: the release must still be immutable with the lock's digest and
/// size for every archive, and the source tarball must still have the lock's sha256.
pub(crate) fn online(
    pandoc: &Pandoc,
    work: &Path,
    problems: &mut Vec<String>,
) -> Result<(), Failure> {
    let dir = work.join("pandoc");
    let release = release_json(&pandoc.version, &dir)?;
    for (triple, archive) in &pandoc.archives {
        let want = Some((archive.sha256.clone(), archive.size));
        if asset_digest(&release, &archive.asset) != want {
            problems.push(format!(
                "{triple}: GitHub no longer gives pandoc's {} sha256 {} and size {}",
                archive.asset, archive.sha256, archive.size
            ));
        }
    }
    let name = source_file(&pandoc.version);
    let path = fetch(&dir, &name, &source_url(&pandoc.version))?;
    let sha = net::sha256_file(&path)
        .map_err(|error| Failure::io("reading pandoc's source tarball", &error))?;
    if sha != pandoc.source_sha256 {
        problems.push(format!(
            "pandoc's source tarball {} now has sha256 {sha}, but the lock pins {}",
            source_url(&pandoc.version),
            pandoc.source_sha256
        ));
    }
    Ok(())
}
