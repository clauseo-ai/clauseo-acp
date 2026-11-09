//! The notices `bump` writes: it unpacks PBS's archives, each wheel's `.dist-info` and pandoc's
//! licence files from the downloads (each checked against the lock's sha256 first) and reads the
//! licence files.

use super::Failure;
use super::bump::Options;
use super::lock::Lock;
use super::net::{self, fetch_verified};
use super::notices::{self, Section};
use super::target::TARGETS;
use crate::output::Printer;
use crate::process::{Cmd, Program};
use crate::write;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// tar unpacks one PBS archive of tens of MB: seconds.
const TAR_TIMEOUT: Duration = Duration::from_mins(5);
/// Python unpacks one wheel's `.dist-info`: a second.
const UNZIP_TIMEOUT: Duration = Duration::from_mins(2);

/// Unpacks only the `.dist-info` folder of the wheel in `argv[1]` into `argv[2]`.
const UNZIP_DIST_INFO: &str = "import sys, zipfile\n\
    z = zipfile.ZipFile(sys.argv[1])\n\
    for n in z.namelist():\n\
    \x20   if n.split('/')[0].endswith('.dist-info'):\n\
    \x20       z.extract(n, sys.argv[2])\n";

/// The notices text for `lock` (whose text is `lock_text`), from the files in `cache`.
pub(crate) fn notices<W: Write>(
    options: &Options,
    lock: &Lock,
    lock_text: &str,
    cache: &Path,
    out: &mut Printer<W>,
) -> Result<String, Failure> {
    out.note("bump: reading the licence files of PBS and of every wheel");
    let mut cpython = Section::new(
        format!("CPython {}", lock.runtime.python),
        String::new(),
        format!(
            "Built by python-build-standalone, release {}.",
            lock.runtime.pbs_release
        ),
    );
    let mut components: BTreeMap<String, Section> = BTreeMap::new();
    let mut pip: Option<Section> = None;
    for target in TARGETS {
        let triple = target.triple();
        let asset = lock.pbs.get(triple).ok_or_else(|| missing(triple))?;
        let dir = cache.join("pbs");
        let full = dir.join(&asset.full_asset);
        let install = dir.join(&asset.asset);
        let unpacked = cache.join("unpacked").join(triple);
        untar(
            &full,
            &unpacked.join("full"),
            &["--zstd", "-xf"],
            &["python/PYTHON.json", "python/licenses"],
        )?;
        untar(&install, &unpacked.join("install"), &["-xzf"], &[])?;
        let base = unpacked.join("full").join("python");
        let json = read_json(&base.join("PYTHON.json"))?;
        for (path, (modules, ids)) in notices::pbs_components(&json) {
            let body = read_text(&base.join(&path))?;
            let file = path.rsplit('/').next().unwrap_or(&path).to_owned();
            if modules.contains("CPython") {
                cpython.licence = ids.iter().cloned().collect::<Vec<_>>().join(" AND ");
                cpython.add(&file, &body, triple);
                continue;
            }
            let name = file
                .trim_start_matches("LICENSE.")
                .trim_end_matches(".txt")
                .to_owned();
            let modules: Vec<String> = modules.into_iter().collect();
            let section = components.entry(name.clone()).or_insert_with(|| {
                Section::new(
                    name.clone(),
                    ids.iter().cloned().collect::<Vec<_>>().join(", "),
                    format!(
                        "Built into CPython by python-build-standalone, for the modules {}.",
                        modules.join(", ")
                    ),
                )
            });
            section.add(&file, &body, triple);
        }
        let pip_dist_info = find_dist_info(&unpacked.join("install").join("python"), "pip-")?;
        let version = pip_dist_info
            .file_name()
            .map(|n| {
                n.to_string_lossy()
                    .trim_start_matches("pip-")
                    .trim_end_matches(".dist-info")
                    .to_owned()
            })
            .unwrap_or_default();
        let section = pip.get_or_insert_with(|| {
            Section::new(
                format!("pip {version}"),
                "MIT".to_owned(),
                "Comes with CPython in PBS's build, with the libraries it vendors.".to_owned(),
            )
        });
        for (path, body) in notices::dist_info_texts(&pip_dist_info)? {
            section.add(&path, &body, triple);
        }
    }
    let mut sections = vec![cpython];
    sections.extend(components.into_values());
    sections.extend(pip);
    for package in &lock.packages {
        let mut section = Section::new(
            notices::package_title(&package.name, &package.version),
            package.licence.clone(),
            String::new(),
        );
        for (triple, wheel) in &package.wheels {
            let target = TARGETS
                .into_iter()
                .find(|t| t.triple() == triple)
                .ok_or_else(|| missing(triple))?;
            let wheels = cache.join("wheels");
            let path = fetch_verified(
                &wheels,
                &wheel.file,
                &wheel.url,
                &wheel.sha256,
                Some(wheel.size),
            )?;
            let dest = cache.join("dist-info").join(&wheel.file);
            if !dest.exists() {
                unzip_dist_info(options, &path, &dest)?;
            }
            let dist_info = find_dist_info(&dest, "")?;
            for (rel, body) in notices::dist_info_texts(&dist_info)? {
                section.add(&rel, &body, target.triple());
            }
        }
        if section.texts.is_empty() {
            return Err(Failure::new(
                format!("{} {} ships no licence file", package.name, package.version),
                "review the package's licence before it goes in the runtime",
            ));
        }
        sections.push(section);
    }
    let texts = super::pandoc::licence_texts(
        &lock.pandoc,
        &TARGETS,
        &options.resolver,
        &cache.join("pandoc"),
    )?;
    sections.push(super::pandoc::section(&lock.pandoc, &texts));
    Ok(notices::render(lock, lock_text, &sections))
}

fn missing(triple: &str) -> Failure {
    Failure::new(
        format!("the lock has no PBS asset for {triple}"),
        "run `cargo xtask python-runtime check`",
    )
}

/// Unpacks `archive` (or only `members` of it) into `dest` with tar.
fn untar(archive: &Path, dest: &Path, flags: &[&str], members: &[&str]) -> Result<(), Failure> {
    if dest.exists() {
        return Ok(());
    }
    write::make_dirs(dest).map_err(|error| Failure::io("making an unpack folder", &error))?;
    let mut args: Vec<String> = flags.iter().map(|f| (*f).to_owned()).collect();
    args.push(archive.to_string_lossy().into_owned());
    args.extend(["-C".to_owned(), dest.to_string_lossy().into_owned()]);
    args.extend(members.iter().map(|m| (*m).to_owned()));
    let cmd = Cmd::new(Program::Named("tar"), args, TAR_TIMEOUT);
    net::run(
        &cmd,
        dest,
        &format!("unpacking {}", archive.display()),
        "bump needs a tar that reads zstd (GNU tar with zstd on PATH, or bsdtar 3.4 or newer)",
    )
    .map(drop)
}

fn unzip_dist_info(options: &Options, wheel: &Path, dest: &Path) -> Result<(), Failure> {
    write::make_dirs(dest).map_err(|error| Failure::io("making an unpack folder", &error))?;
    let cmd = Cmd::new(
        Program::Path(options.resolver.clone()),
        [
            "-I".to_owned(),
            "-c".to_owned(),
            UNZIP_DIST_INFO.to_owned(),
            wheel.to_string_lossy().into_owned(),
            dest.to_string_lossy().into_owned(),
        ],
        UNZIP_TIMEOUT,
    );
    net::run(
        &cmd,
        dest,
        &format!("unpacking {}", wheel.display()),
        "check the Python passed as --resolver-python",
    )
    .map(drop)
}

/// The one `<prefix>*.dist-info` folder under `root`, at any depth.
fn find_dist_info(root: &Path, prefix: &str) -> Result<PathBuf, Failure> {
    let found: Vec<String> = super::tree::walk(root)?
        .into_iter()
        .filter(|(rel, kind, _)| {
            let name = rel.rsplit('/').next().unwrap_or(rel);
            *kind == super::tree::Kind::Dir
                && name.ends_with(".dist-info")
                && name.starts_with(prefix)
        })
        .map(|(rel, _, _)| rel)
        .collect();
    match found.as_slice() {
        [one] => Ok(root.join(one)),
        _ => Err(Failure::new(
            format!(
                "{} holds {} {prefix}*.dist-info folders, not one",
                root.display(),
                found.len()
            ),
            "check the archive's layout",
        )),
    }
}

fn read_text(path: &Path) -> Result<String, Failure> {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|error| Failure::io(&format!("reading {}", path.display()), &error))
}

fn read_json(path: &Path) -> Result<serde_json::Value, Failure> {
    let bytes = std::fs::read(path)
        .map_err(|error| Failure::io(&format!("reading {}", path.display()), &error))?;
    serde_json::from_slice(&bytes).map_err(|error| {
        Failure::new(
            format!("{} is not JSON: {error}", path.display()),
            "check the PBS archive",
        )
    })
}
