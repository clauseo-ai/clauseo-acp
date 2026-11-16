//! tesseract in the runtime: the `tesseract` OCR program, which `build` compiles on each target
//! from the source tarballs `[tesseract]` of the lock pins by sha256 (tesseract and leptonica,
//! nothing else, linked statically; `tesseract_build.rs`), and its trained models from
//! `tesseract-ocr/tessdata_fast` at a pinned commit. The program goes where pandoc goes, `bin/` on
//! macOS and `clauseo-bin\` on Windows, with the models in a `tessdata` folder beside it.
//!
//! Why built here and not fetched like pandoc: neither project publishes a portable binary for
//! these targets. The packaged ones need macOS 15 (Homebrew's bottles, tesserocr's wheels), pull in
//! LGPL/GPL libraries (Homebrew, conda-forge), or are an installer (UB Mannheim). So the
//! sources are pinned instead, the way pandoc's archives are: `bump` takes leptonica's sha256 from
//! GitHub's API (`digest` of the release asset) and hashes what it downloads for the rest, `build`
//! refuses any download whose sha256 or size differs, and `check --online` re-hashes every pin
//! nightly. Neither release is immutable, so there is no `gh release verify-asset` here; the
//! sha256 pins are the check.
//!
//! Finding the models. tesseract looks for them in `--tessdata-dir`, then `TESSDATA_PREFIX`, then
//! (Windows only) a `tessdata` folder beside the program; on macOS there is no such fallback, and
//! no prefix is compiled in. So callers pass `--tessdata-dir <the program's folder>/tessdata` on
//! every OS, and nothing needs an environment variable. The smoke test checks both ways.
//!
//! Licences: tesseract Apache-2.0, leptonica BSD-2-Clause, the `tessdata_fast` models Apache-2.0,
//! all in the packages' allowlist (`wheel::licence_allowed`). The notices carry tesseract's
//! `LICENSE`, leptonica's `leptonica-license.txt` and `tessdata_fast`'s `LICENSE`, word for word.

use super::Failure;
use super::lock::{Lock, Model, Tesseract};
use super::net::{self, fetch, fetch_json, fetch_verified};
use super::notices::Section;
use super::rules::is_sha256;
use super::target::{TARGETS, Target};
use super::wheel;
use crate::write;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// tesseract's repository.
pub(crate) const REPO: &str = "tesseract-ocr/tesseract";
/// leptonica's repository.
pub(crate) const LEPTONICA_REPO: &str = "DanBloomberg/leptonica";
/// The models' repository.
pub(crate) const TESSDATA_REPO: &str = "tesseract-ocr/tessdata_fast";
/// tesseract's licence (its `LICENSE`).
pub(crate) const LICENCE: &str = "Apache-2.0";
/// leptonica's licence (its `leptonica-license.txt`).
pub(crate) const LEPTONICA_LICENCE: &str = "BSD-2-Clause";
/// The models' licence (`tessdata_fast`'s `LICENSE`).
pub(crate) const TESSDATA_LICENCE: &str = "Apache-2.0";
/// The folder under `downloads` (or the bump cache) the pinned files go to.
pub(crate) const DIR: &str = "tesseract";

/// One pinned download.
pub(crate) struct Pin {
    /// Its file name in the download folder.
    pub file: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// The tag's source tarball.
pub(crate) fn source_url(version: &str) -> String {
    format!("https://github.com/{REPO}/archive/refs/tags/{version}.tar.gz")
}

/// leptonica's release tarball.
pub(crate) fn leptonica_url(version: &str) -> String {
    format!(
        "https://github.com/{LEPTONICA_REPO}/releases/download/{version}/leptonica-{version}.tar.gz"
    )
}

/// A file of `tessdata_fast` at `commit`.
pub(crate) fn tessdata_url(commit: &str, file: &str) -> String {
    format!("https://raw.githubusercontent.com/{TESSDATA_REPO}/{commit}/{file}")
}

/// The folder the program and its `tessdata` go in, relative to the runtime's root.
pub(crate) fn bin_dir(target: Target) -> &'static str {
    if target.is_macos() {
        "bin"
    } else {
        "clauseo-bin"
    }
}

/// The program's file name.
pub(crate) fn exe(target: Target) -> &'static str {
    if target.is_macos() {
        "tesseract"
    } else {
        "tesseract.exe"
    }
}

/// tesseract's file name in a download folder.
pub(crate) fn source_file(version: &str) -> String {
    format!("tesseract-{version}-source.tar.gz")
}

/// leptonica's file name in a download folder.
pub(crate) fn leptonica_file(version: &str) -> String {
    format!("leptonica-{version}.tar.gz")
}

/// The first 7 digits of the tessdata commit, for titles.
fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

/// Every pinned download, in a fixed order: tesseract, leptonica, tessdata's `LICENSE`, then each
/// model.
pub(crate) fn pins(t: &Tesseract) -> Vec<Pin> {
    let mut pins = vec![
        Pin {
            file: source_file(&t.version),
            url: source_url(&t.version),
            sha256: t.source_sha256.clone(),
            size: t.source_size,
        },
        Pin {
            file: leptonica_file(&t.leptonica),
            url: leptonica_url(&t.leptonica),
            sha256: t.leptonica_sha256.clone(),
            size: t.leptonica_size,
        },
        Pin {
            file: format!("tessdata_fast-{}-LICENSE", short(&t.tessdata_commit)),
            url: tessdata_url(&t.tessdata_commit, "LICENSE"),
            sha256: t.tessdata_licence_sha256.clone(),
            size: t.tessdata_licence_size,
        },
    ];
    for (language, model) in &t.models {
        pins.push(Pin {
            file: model_file(&t.tessdata_commit, language),
            url: tessdata_url(&t.tessdata_commit, &format!("{language}.traineddata")),
            sha256: model.sha256.clone(),
            size: model.size,
        });
    }
    pins
}

/// A model's file name in a download folder.
pub(crate) fn model_file(commit: &str, language: &str) -> String {
    format!("tessdata_fast-{}-{language}.traineddata", short(commit))
}

fn release_ok(version: &str) -> bool {
    !version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

fn commit_ok(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Every problem with `[tesseract]`, added to `found`.
pub(crate) fn problems(lock: &Lock, found: &mut Vec<String>) {
    let t = &lock.tesseract;
    if !release_ok(&t.version) {
        found.push(format!(
            "tesseract.version is `{}`, not a release (5.5.4)",
            t.version
        ));
    }
    if !release_ok(&t.leptonica) {
        found.push(format!(
            "tesseract.leptonica is `{}`, not a release (1.87.0)",
            t.leptonica
        ));
    }
    if !commit_ok(&t.commit) {
        found.push("tesseract.commit is not a 40-digit lowercase commit id".to_owned());
    }
    if !commit_ok(&t.tessdata_commit) {
        found.push("tesseract.tessdata_commit is not a 40-digit lowercase commit id".to_owned());
    }
    let mut sorted = t.languages.clone();
    sorted.sort_unstable();
    sorted.dedup();
    if t.languages.is_empty() || sorted != t.languages {
        found.push("tesseract.languages is empty, not sorted, or repeats a language".to_owned());
    }
    for language in &t.languages {
        let ok = !language.is_empty()
            && language
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_');
        if !ok {
            found.push(format!(
                "tesseract.languages has `{language}`, not a tesseract language code"
            ));
        }
    }
    let models: Vec<&String> = t.models.keys().collect();
    if models != t.languages.iter().collect::<Vec<_>>() {
        found.push(
            "[tesseract.models] must have one entry per language of tesseract.languages".to_owned(),
        );
    }
    for pin in pins(t) {
        if !is_sha256(&pin.sha256) || pin.size == 0 {
            found.push(format!(
                "[tesseract] has no sha256 or size for {}",
                pin.file
            ));
        }
    }
    for licence in [LICENCE, LEPTONICA_LICENCE, TESSDATA_LICENCE] {
        if let Err(why) = wheel::licence_allowed(licence) {
            found.push(format!("tesseract: {why}"));
        }
    }
}

/// `build`'s step 1 for tesseract: every pinned file into `<dir>/tesseract`, each against the
/// lock's sha256 and size. Returns that folder.
pub(crate) fn inputs(t: &Tesseract, dir: &Path) -> Result<PathBuf, Failure> {
    let dir = dir.join(DIR);
    for pin in pins(t) {
        fetch_verified(&dir, &pin.file, &pin.url, &pin.sha256, Some(pin.size))?;
    }
    Ok(dir)
}

/// The sha256 and size of a downloaded file.
fn hash(path: &Path) -> Result<(String, u64), Failure> {
    let what = format!("reading {}", path.display());
    let sha = net::sha256_file(path).map_err(|error| Failure::io(&what, &error))?;
    let size = path
        .metadata()
        .map_err(|error| Failure::io(&what, &error))?
        .len();
    Ok((sha, size))
}

/// Downloads `url` afresh into `<dir>/<file>` and hashes it.
fn fetch_fresh(dir: &Path, file: &str, url: &str) -> Result<(String, u64), Failure> {
    if dir.join(file).exists() {
        net::remove(dir, file)?;
    }
    hash(&fetch(dir, file, url)?)
}

/// The sha256 (from `digest`) and size GitHub's API gives leptonica's release tarball.
fn leptonica_digest(version: &str, dir: &Path) -> Result<(String, u64), Failure> {
    let url = format!("https://api.github.com/repos/{LEPTONICA_REPO}/releases/tags/{version}");
    let json = fetch_json(dir, &format!("leptonica-release-{version}.json"), &url)?;
    let asset = leptonica_file(version);
    json.get("assets")
        .and_then(serde_json::Value::as_array)
        .and_then(|assets| {
            assets
                .iter()
                .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(asset.as_str()))
        })
        .and_then(|entry| {
            let sha = entry.get("digest")?.as_str()?.strip_prefix("sha256:")?;
            Some((sha.to_owned(), entry.get("size")?.as_u64()?))
        })
        .ok_or_else(|| {
            Failure::new(
                format!("leptonica {version}'s release has no asset {asset} with a digest"),
                "check tesseract.leptonica in the lock",
            )
        })
}

/// `bump`'s `[tesseract]`: `old`'s hand-edited fields, with the commit and every sha256 and size
/// taken afresh into `<cache>/tesseract`.
pub(crate) fn bump(old: &Tesseract, cache: &Path) -> Result<Tesseract, Failure> {
    let dir = cache.join(DIR);
    write::make_dirs(&dir).map_err(|error| Failure::io("making the cache folder", &error))?;
    let commit_url = format!(
        "https://api.github.com/repos/{REPO}/commits/{}",
        old.version
    );
    let commit = fetch_json(&dir, &format!("commit-{}.json", old.version), &commit_url)?
        .get("sha")
        .and_then(|sha| sha.as_str())
        .map(str::to_owned)
        .ok_or_else(|| Failure::new(format!("{commit_url} gives no sha"), "try again later"))?;
    let (source_sha256, source_size) =
        fetch_fresh(&dir, &source_file(&old.version), &source_url(&old.version))?;
    let (leptonica_sha256, leptonica_size) = leptonica_digest(&old.leptonica, &dir)?;
    fetch_verified(
        &dir,
        &leptonica_file(&old.leptonica),
        &leptonica_url(&old.leptonica),
        &leptonica_sha256,
        Some(leptonica_size),
    )?;
    let mut new = Tesseract {
        commit,
        source_sha256,
        source_size,
        leptonica_sha256,
        leptonica_size,
        models: BTreeMap::new(),
        ..old.clone()
    };
    let licence = format!("tessdata_fast-{}-LICENSE", short(&old.tessdata_commit));
    let licence_url = tessdata_url(&old.tessdata_commit, "LICENSE");
    (new.tessdata_licence_sha256, new.tessdata_licence_size) =
        fetch_fresh(&dir, &licence, &licence_url)?;
    for language in &old.languages {
        let (sha256, size) = fetch_fresh(
            &dir,
            &model_file(&old.tessdata_commit, language),
            &tessdata_url(&old.tessdata_commit, &format!("{language}.traineddata")),
        )?;
        new.models.insert(language.clone(), Model { sha256, size });
    }
    Ok(new)
}

/// `check --online` for tesseract: every pinned file must still download with the lock's sha256
/// and size, and GitHub must still give leptonica's tarball that digest.
pub(crate) fn online(
    t: &Tesseract,
    work: &Path,
    problems: &mut Vec<String>,
) -> Result<(), Failure> {
    let dir = work.join(DIR);
    write::make_dirs(&dir).map_err(|error| Failure::io("making a scratch folder", &error))?;
    for pin in pins(t) {
        let found = fetch_fresh(&dir, &pin.file, &pin.url)?;
        if found != (pin.sha256.clone(), pin.size) {
            problems.push(format!(
                "{} now has sha256 {} and size {}, but the lock pins {} and {}",
                pin.url, found.0, found.1, pin.sha256, pin.size
            ));
        }
    }
    let digest = leptonica_digest(&t.leptonica, &dir)?;
    if digest != (t.leptonica_sha256.clone(), t.leptonica_size) {
        problems.push(format!(
            "GitHub no longer gives leptonica's {} sha256 {} and size {}",
            leptonica_file(&t.leptonica),
            t.leptonica_sha256,
            t.leptonica_size
        ));
    }
    Ok(())
}

/// The licence texts, read with `python` from the pinned files in `<dir>/tesseract` (each checked
/// against the lock first): tesseract's, leptonica's and `tessdata_fast`'s, as `(path, body)`.
/// Unpacks into `<dir>/tesseract/licences`, made afresh.
pub(crate) fn licence_texts(
    t: &Tesseract,
    python: &str,
    dir: &Path,
) -> Result<[(String, String); 3], Failure> {
    let folder = inputs(t, dir)?;
    let out = folder.join("licences");
    if out.exists() {
        net::remove(&folder, "licences")?;
    }
    write::make_dirs(&out).map_err(|error| Failure::io("making an unpack folder", &error))?;
    let tess_member = format!("tesseract-{}/LICENSE", t.version);
    let lept_member = format!("leptonica-{}/leptonica-license.txt", t.leptonica);
    let tess_dest = out.join("tesseract-LICENSE");
    let lept_dest = out.join("leptonica-license.txt");
    super::pandoc::extract(
        python,
        &folder.join(source_file(&t.version)),
        &[(&tess_member, &tess_dest, "644")],
        &folder,
    )?;
    super::pandoc::extract(
        python,
        &folder.join(leptonica_file(&t.leptonica)),
        &[(&lept_member, &lept_dest, "644")],
        &folder,
    )?;
    let tessdata = folder.join(format!(
        "tessdata_fast-{}-LICENSE",
        short(&t.tessdata_commit)
    ));
    Ok([
        (tess_member, read_text(&tess_dest)?),
        (lept_member, read_text(&lept_dest)?),
        ("tessdata_fast/LICENSE".to_owned(), read_text(&tessdata)?),
    ])
}

fn read_text(path: &Path) -> Result<String, Failure> {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|error| Failure::io(&format!("reading {}", path.display()), &error))
}

/// The section titles and licences in the notices, in order.
pub(crate) fn titles(t: &Tesseract) -> [(String, &'static str); 3] {
    [
        (format!("tesseract {}", t.version), LICENCE),
        (format!("leptonica {}", t.leptonica), LEPTONICA_LICENCE),
        (
            format!("tessdata_fast {}", short(&t.tessdata_commit)),
            TESSDATA_LICENCE,
        ),
    ]
}

/// The notices sections of tesseract, leptonica and the models, from `licence_texts`.
pub(crate) fn sections(t: &Tesseract, texts: &[(String, String); 3]) -> Vec<Section> {
    let notes = [
        format!(
            "The tesseract program, built by Clauseo with leptonica linked in statically: \
             bin/tesseract on\nmacOS, clauseo-bin\\tesseract.exe on Windows. No training tools, \
             libarchive, libcurl, libtiff or\nOpenCL. Built from the source tarball of the tag {} \
             (commit {}):\n{}\n(sha256 {}).",
            t.version,
            t.commit,
            source_url(&t.version),
            t.source_sha256
        ),
        format!(
            "Linked statically into tesseract, with every image codec off. Built from\n{}\n\
             (sha256 {}).",
            leptonica_url(&t.leptonica),
            t.leptonica_sha256
        ),
        format!(
            "The trained models {}, unchanged, from https://github.com/{TESSDATA_REPO}\nat commit \
             {}: bin/tessdata/ on macOS, clauseo-bin\\tessdata\\ on Windows.",
            t.languages
                .iter()
                .map(|l| format!("{l}.traineddata"))
                .collect::<Vec<_>>()
                .join(", "),
            t.tessdata_commit
        ),
    ];
    titles(t)
        .into_iter()
        .zip(notes)
        .zip(texts)
        .map(|(((title, licence), note), (path, body))| {
            let mut section = Section::new(title, licence.to_owned(), note);
            for target in TARGETS {
                section.add(path, body, target.triple());
            }
            section
        })
        .collect()
}

/// `bump`'s notices sections for `[tesseract]`: `sections` of `licence_texts`.
pub(crate) fn notice_sections(
    t: &Tesseract,
    python: &str,
    dir: &Path,
) -> Result<Vec<Section>, Failure> {
    Ok(sections(t, &licence_texts(t, python, dir)?))
}

/// Every problem with the notices for `[tesseract]`: a section it does not list, or a source it
/// does not point at.
pub(crate) fn notice_problems(notices: &str, t: &Tesseract, found: &mut Vec<String>) {
    for (title, licence) in titles(t) {
        let line = format!("- {title} ({licence})");
        if !notices.lines().any(|l| l == line) {
            found.push(format!("THIRD-PARTY-NOTICES.txt does not list `{line}`"));
        }
    }
    for url in [source_url(&t.version), leptonica_url(&t.leptonica)] {
        if !notices.contains(&url) {
            found.push(format!("THIRD-PARTY-NOTICES.txt does not give {url}"));
        }
    }
}

#[cfg(test)]
mod tests;
