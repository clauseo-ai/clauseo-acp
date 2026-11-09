//! `cargo xtask python-runtime build --target <triple> --out <dir>`: assemble one target's archive
//! on a machine of that target, from the lock's inputs only, each checked before it is read.
//!
//! The steps, in order (`run`):
//! 1. inputs: PBS's `SHA256SUMS` must give the asset the lock's sha256; the asset and every wheel
//!    are downloaded to `<out>/downloads` and must have the lock's sha256 and size; PBS's Sigstore
//!    build attestation is checked with `gh attestation verify` (fails closed). `--download-only`
//!    stops here, on any machine.
//! 2. unpack PBS into `<out>/stage/python`; macOS symlinks become copies of their files, and no
//!    symlink may remain anywhere (Clauseo's extractor skips links).
//! 3. pip installs the wheels with the runtime's own pip: `--no-index --find-links --require-hashes
//!    --only-binary=:all: --no-deps`, then `pip check` proves the closure. Scripts pip writes
//!    (absolute paths of this machine) are removed.
//! 4. trim (`tree::MACOS_TRIM`, `tree::WINDOWS_TRIM`), and numpy's and pandas's own test suites
//!    (`tree::PACKAGES_WITHOUT_TESTS`).
//! 5. the overlay: PR 2's hook (`overlay`).
//! 6. notices: every licence file in the runtime must be in `THIRD-PARTY-NOTICES.txt`, which is
//!    copied to the runtime's root.
//! 7. `compileall --invalidation-mode unchecked-hash`, so nothing writes `__pycache__` later.
//! 8. the smoke test (`python-runtime/smoke/smoke.py`), which must leave the runtime unchanged.
//! 9. pack a deterministic `.tar.gz` (`python-runtime/tools/pack.py`, run by the runtime itself)
//!    and print its name, sha256, size and unpacked size, also written to `<out>/build.json`.

use super::lock::Lock;
use super::net::{self, fetch_verified};
use super::notices;
use super::target::Target;
use super::tree::{self, Kind};
use super::{Failure, NOTICES, PACK, SMOKE, SMOKE_DIR};
use crate::output::Printer;
use crate::process::{Cmd, Program};
use crate::write;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// tar unpacks the PBS archive: seconds.
const TAR_TIMEOUT: Duration = Duration::from_mins(5);
/// pip installs about twenty local wheels: a minute at most.
const PIP_TIMEOUT: Duration = Duration::from_mins(10);
/// compileall compiles the whole runtime on every core: a minute or two.
const COMPILE_TIMEOUT: Duration = Duration::from_mins(15);
/// The smoke test imports everything and round-trips four small files: under a minute.
const SMOKE_TIMEOUT: Duration = Duration::from_mins(10);
/// Packing and compressing about 200 MB: a few minutes.
const PACK_TIMEOUT: Duration = Duration::from_mins(20);

/// What `build` was asked for.
pub(crate) struct Options {
    pub target: Target,
    pub out: PathBuf,
    /// Stop after the inputs are downloaded and checked.
    pub download_only: bool,
    /// Skip `gh attestation verify`: a local try without gh only, never CI.
    pub no_attestation: bool,
}

/// Runs the steps the module doc lists.
pub(crate) fn run<W: Write>(
    root: &Path,
    lock: &Lock,
    options: &Options,
    out: &mut Printer<W>,
) -> Result<(), Failure> {
    let target = options.target;
    if !options.download_only && Target::host() != Some(target) {
        return Err(Failure::new(
            format!(
                "this machine is not {}, so it cannot build that target's runtime",
                target.triple()
            ),
            "run it on a machine of that target (CI's matrix), or pass --download-only to only \
             download and check the inputs",
        ));
    }
    let out_dir = absolute(&options.out)?;
    let downloads = out_dir.join("downloads");
    let asset = inputs(lock, options, &downloads, out)?;
    if options.download_only {
        out.note("build: --download-only, so it stops with the inputs checked");
        return Ok(());
    }
    let stage = out_dir.join("stage");
    if stage.exists() {
        net::remove(&out_dir, "stage")?;
    }
    write::make_dirs(&stage).map_err(|error| Failure::io("making the stage folder", &error))?;
    let tar = Cmd::new(
        Program::Named("tar"),
        ["-xzf", &format!("downloads/{asset}"), "-C", "stage"],
        TAR_TIMEOUT,
    );
    net::run(
        &tar,
        &out_dir,
        "unpacking the PBS archive",
        "check that tar is on PATH",
    )?;
    let runtime = stage.join("python");
    let python = runtime.join(if target.is_macos() {
        "bin/python3.13"
    } else {
        "python.exe"
    });
    if !python.is_file() {
        return Err(Failure::new(
            format!("the PBS archive has no {}", python.display()),
            "check the PBS archive's layout",
        ));
    }
    if target.is_macos() {
        for done in tree::materialize_symlinks(&runtime)? {
            out.note(format!("build: symlink replaced by a copy: {done}"));
        }
    }
    let links = tree::symlinks(&runtime)?;
    if !links.is_empty() {
        return Err(Failure::new(
            format!("the runtime still holds symlinks: {}", links.join(", ")),
            "replace them with files before packing; the extractor skips links",
        ));
    }
    install_wheels(lock, target, &runtime, &python, &downloads, out)?;
    let rules: &[tree::Trim] = if target.is_macos() {
        &tree::MACOS_TRIM
    } else {
        &tree::WINDOWS_TRIM
    };
    let mut removed = tree::apply_trim(&runtime, rules)?;
    let site = if target.is_macos() {
        "lib/python3.13/site-packages"
    } else {
        "Lib/site-packages"
    };
    removed.extend(tree::drop_package_tests(
        &runtime.join(site),
        &tree::PACKAGES_WITHOUT_TESTS,
    )?);
    out.note(format!("build: trimmed {}", removed.join(", ")));
    overlay(&runtime, out);
    ship_notices(root, &runtime)?;
    let compile = python_cmd(
        &python,
        &[
            "-m",
            "compileall",
            "-q",
            "-f",
            "-j",
            "0",
            "--invalidation-mode",
            "unchecked-hash",
        ],
        &[&runtime],
        COMPILE_TIMEOUT,
    );
    net::run(
        &compile,
        &out_dir,
        "compiling the runtime's .py files",
        "read compileall's output",
    )?;
    smoke(root, &runtime, &python, &out_dir, out)?;
    pack(root, lock, target, &runtime, &python, &out_dir, out)
}

/// `path` made absolute against the working directory, since every command runs elsewhere.
fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    std::path::absolute(path)
        .map_err(|error| Failure::io(&format!("resolving {}", path.display()), &error))
}

/// Step 1: downloads and checks PBS's asset and the wheels. Returns the asset's file name.
fn inputs<W: Write>(
    lock: &Lock,
    options: &Options,
    downloads: &Path,
    out: &mut Printer<W>,
) -> Result<String, Failure> {
    let triple = options.target.triple();
    let pbs = lock.pbs.get(triple).ok_or_else(|| {
        Failure::new(
            format!("the lock has no [pbs.{triple}]"),
            "run `cargo xtask python-runtime check`",
        )
    })?;
    let release = &lock.runtime.pbs_release;
    net::check_pbs_sums(downloads, release, &pbs.asset, &pbs.sha256)?;
    let file = fetch_verified(
        downloads,
        &pbs.asset,
        &net::pbs_url(release, &pbs.asset),
        &pbs.sha256,
        Some(pbs.size),
    )?;
    out.note(format!(
        "build: {} matches the lock and PBS's SHA256SUMS",
        pbs.asset
    ));
    if options.no_attestation {
        out.note("build: --no-attestation: PBS's build attestation was NOT checked");
    } else {
        net::verify_pbs_attestation(&file)?;
        out.note(format!(
            "build: PBS's build attestation of {} holds",
            pbs.asset
        ));
    }
    let wheels = downloads.join("wheels");
    for package in &lock.packages {
        if let Some(wheel) = package.wheels.get(triple) {
            fetch_verified(
                &wheels,
                &wheel.file,
                &wheel.url,
                &wheel.sha256,
                Some(wheel.size),
            )?;
        }
    }
    out.note(format!("build: every wheel for {triple} matches the lock"));
    Ok(pbs.asset.clone())
}

/// A command that runs the runtime's Python isolated from the environment (`-I`), without
/// writing bytecode (`-B`).
fn python_cmd(python: &Path, args: &[&str], paths: &[&Path], timeout: Duration) -> Cmd {
    let mut all = vec!["-I".to_owned(), "-B".to_owned()];
    all.extend(args.iter().map(|arg| (*arg).to_owned()));
    all.extend(paths.iter().map(|path| path.to_string_lossy().into_owned()));
    Cmd::new(
        Program::Path(python.to_string_lossy().into_owned()),
        all,
        timeout,
    )
}

/// Step 3: the runtime's pip installs exactly the lock's wheels for `target`, by hash, offline.
fn install_wheels<W: Write>(
    lock: &Lock,
    target: Target,
    runtime: &Path,
    python: &Path,
    downloads: &Path,
    out: &mut Printer<W>,
) -> Result<(), Failure> {
    let mut lines: Vec<String> = lock
        .packages
        .iter()
        .filter_map(|package| {
            let wheel = package.wheels.get(target.triple())?;
            Some(format!(
                "{}=={} --hash=sha256:{}",
                package.name, package.version, wheel.sha256
            ))
        })
        .collect();
    lines.push(String::new());
    let requirements = lines.join("\n");
    let name = format!("requirements-{}.txt", target.triple());
    write::write_file_in(downloads, &name, requirements.as_bytes())
        .map_err(|error| Failure::io("writing the requirements file", &error))?;
    let scripts = runtime.join(if target.is_macos() { "bin" } else { "Scripts" });
    let before = tree::names_in(&scripts);
    let wheels = downloads.join("wheels");
    let install = python_cmd(
        python,
        &[
            "-m",
            "pip",
            "install",
            "--isolated",
            "--no-index",
            "--require-hashes",
            "--only-binary=:all:",
            "--no-deps",
            "--no-compile",
            "--no-cache-dir",
            "--disable-pip-version-check",
            "--no-warn-script-location",
            "--find-links",
        ],
        &[&wheels, Path::new("-r"), &downloads.join(&name)],
        PIP_TIMEOUT,
    );
    net::run(
        &install,
        downloads,
        "installing the wheels",
        "read pip's output above",
    )?;
    for added in tree::names_in(&scripts) {
        if !before.contains(&added) {
            net::remove(&scripts, &added)?;
            out.note(format!("build: removed the script pip wrote: {added}"));
        }
    }
    let check = python_cmd(
        python,
        &["-m", "pip", "check", "--disable-pip-version-check"],
        &[],
        PIP_TIMEOUT,
    );
    net::run(
        &check,
        downloads,
        "pip check",
        "a dependency is missing from the lock: run bump",
    )?;
    out.note(format!(
        "build: installed the wheels for {}; pip check passes",
        target.triple()
    ));
    Ok(())
}

/// Step 5, PR 2's hook: the overlay (`sitecustomize.py`, `clauseo_pip`, the site `pip.conf`, and on
/// Windows the `clauseo-bin` launcher) goes into `runtime` here, after the trim and before the
/// notices, compileall and the smoke test, so its files are compiled, smoke-tested and packed
/// like the rest. Nothing yet.
fn overlay<W: Write>(runtime: &Path, out: &mut Printer<W>) {
    out.note(format!(
        "build: overlay: none yet (PR 2 adds it to {})",
        runtime.display()
    ));
}

/// Step 6: every licence file under `runtime` must be in the notices; then the notices go to its
/// root.
fn ship_notices(root: &Path, runtime: &Path) -> Result<(), Failure> {
    let text = std::fs::read_to_string(root.join(NOTICES))
        .map_err(|error| Failure::io(&format!("reading {NOTICES}"), &error))?;
    for (rel, kind, _) in tree::walk(runtime)? {
        if kind != Kind::Dir || !rel.ends_with(".dist-info") {
            continue;
        }
        for (path, body) in notices::dist_info_texts(&runtime.join(&rel))? {
            if !notices::covers(&text, &body) {
                return Err(Failure::new(
                    format!("{NOTICES} does not hold the licence file {path} of the runtime"),
                    "run `cargo xtask python-runtime bump` to write the notices again",
                ));
            }
        }
    }
    write::write_file_in(runtime, "THIRD-PARTY-NOTICES.txt", text.as_bytes())
        .map_err(|error| Failure::io("copying the notices into the runtime", &error))
}

/// Step 8: the smoke test, which must not change a file of the runtime.
fn smoke<W: Write>(
    root: &Path,
    runtime: &Path,
    python: &Path,
    out_dir: &Path,
    out: &mut Printer<W>,
) -> Result<(), Failure> {
    let before = tree::walk(runtime)?;
    let work = out_dir.join("smoke");
    if work.exists() {
        net::remove(out_dir, "smoke")?;
    }
    write::make_dirs(&work).map_err(|error| Failure::io("making the smoke folder", &error))?;
    let cmd = python_cmd(
        python,
        &[],
        &[&root.join(SMOKE), runtime, &root.join(SMOKE_DIR), &work],
        SMOKE_TIMEOUT,
    );
    let said = net::run(&cmd, out_dir, "the smoke test", "read its output above")?;
    out.note(format!("build: smoke test passed:\n{}", said.trim_end()));
    if tree::walk(runtime)? != before {
        return Err(Failure::new(
            "the smoke test changed files in the runtime".to_owned(),
            "find what wrote there (a __pycache__ compileall missed?) and fix it",
        ));
    }
    net::remove(out_dir, "smoke")
}

/// Step 9: packs the runtime and reports the archive.
fn pack<W: Write>(
    root: &Path,
    lock: &Lock,
    target: Target,
    runtime: &Path,
    python: &Path,
    out_dir: &Path,
    out: &mut Printer<W>,
) -> Result<(), Failure> {
    let name = lock.archive(target.os_arch());
    let folder = lock.folder(target.os_arch());
    let archive = out_dir.join(&name);
    if archive.exists() {
        net::remove(out_dir, &name)?;
    }
    let cmd = python_cmd(
        python,
        &[],
        &[&root.join(PACK), runtime, Path::new(&folder), &archive],
        PACK_TIMEOUT,
    );
    net::run(
        &cmd,
        out_dir,
        "packing the archive",
        "read pack.py's output above",
    )?;
    let sha256 = net::sha256_file(&archive)
        .map_err(|error| Failure::io(&format!("reading {name}"), &error))?;
    let size = archive
        .metadata()
        .map_err(|error| Failure::io(&format!("reading {name}"), &error))?
        .len();
    let (files, unpacked) = tree::file_count_and_size(runtime)?;
    let json = serde_json::json!({
        "archive": name, "folder": folder, "target": target.triple(), "sha256": sha256,
        "size": size, "unpacked_size": unpacked, "files": files, "release": lock.release_tag(),
    });
    write::write_file_in(out_dir, "build.json", format!("{json:#}\n").as_bytes())
        .map_err(|error| Failure::io("writing build.json", &error))?;
    out.note(format!("archive: {name}"));
    out.note(format!("sha256: {sha256}"));
    out.note(format!("size: {size}"));
    out.note(format!("unpacked_size: {unpacked} ({files} files)"));
    Ok(())
}
