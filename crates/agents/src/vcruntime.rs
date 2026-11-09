//! The VC++ runtime DLL Codex's ARM64 `rg.exe` imports, placed beside it (Windows ARM64 only).
//!
//! `@openai/codex`'s `win32-arm64` package ships the official ripgrep 15.2.0
//! `aarch64-pc-windows-msvc` `rg.exe` in `vendor/aarch64-pc-windows-msvc/codex-path/`. Unlike
//! the x64 build, it links the VC++ runtime dynamically (ripgrep's `.cargo/config.toml` sets
//! `+crt-static` for x86_64 only) and imports `VCRUNTIME140.dll`, which a clean Windows 11 ARM64
//! install does not have: `rg` fails to start (0xC0000135) and Codex's search with it. `rg.exe`
//! is never changed or rebuilt (Smart App Control pins its bytes, and an unsigned rebuild has no
//! reputation); instead the runtime DLLs it imports are copied beside it, the app-local
//! deployment Microsoft supports.
//!
//! The source is the copy the same package already ships for Codex's voice host,
//! `vendor/aarch64-pc-windows-msvc/codex-resources/voice/bin/vcruntime140.dll` (14.50.35719.0
//! in 0.162.1). It is copied (never hard-linked: a Codex update replaces the source) to a
//! temporary name beside `rg.exe`, and the copy is checked there, so no other file can be
//! swapped in between the check and the rename to its real name:
//! - an ARM64 PE (machine `0xAA64`);
//! - an Authenticode signature `WinVerifyTrust` (`WINTRUST_ACTION_GENERIC_VERIFY_V2`) accepts,
//!   whose signer's organisation is `Microsoft Corporation` and whose chain ends at a Microsoft
//!   root (`CERT_CHAIN_POLICY_MICROSOFT_ROOT`); a hash pin alone would not say who made it;
//! - a VC++ 14.x runtime (its version resource) at least as new as the toolset `rg.exe` was
//!   linked with (its linker version, 14.44 for ripgrep 15.2.0).
//!
//! A copy that fails a check is removed, and Codex installs as before, with `rg` unable to
//! start; the reason is logged on the `clauseo_agents::lifecycle` target. There is no second
//! source: Microsoft publishes no ARM64 `vcruntime140.dll` as a pinnable file of its own (the
//! `vc_redist.arm64.exe` bundle would have to be unpacked).
//!
//! Only when the install is for Windows ARM64, `rg.exe` imports the DLL, and no file of that
//! name (any case) is beside it already. Each DLL is placed or refused alone; nothing here ever
//! fails the install. It runs once per install, in the temp folder before the kept-tree marker,
//! so the marker's manifest (and the receipt after it) include the DLL.
//!
//! Smart App Control (`sac`, PR #812): there `codex-path` is renamed to `codex-path.sac-off`
//! (and back) as a whole folder, so a DLL placed beside `rg.exe` moves with it and stays beside
//! it in every state. This step looks in both folders, so it places the DLL whichever of the two
//! holds `rg.exe` when it runs, before or after `sac::settle`. Its `rg.exe` pin is untouched.

use std::path::{Path, PathBuf};

use clauseo_protocol::AgentId;

use crate::config::AgentsConfig;
use crate::platform::{Arch, Os, Platform};
use crate::write;

pub(crate) mod pe;
#[cfg(windows)]
mod windows;

/// Codex's ARM64 vendor folder, `/`-separated below an install folder.
const VENDOR: &str = "node_modules/@openai/codex-win32-arm64/vendor/aarch64-pc-windows-msvc";
/// The folders that may hold `rg.exe`: `codex-path` (`codex-package.json`'s `pathDir`), and
/// `codex-path.sac-off`, the name Smart App Control's step moves it to (`sac`, PR #812).
const RG_DIRS: [&str; 2] = ["codex-path", "codex-path.sac-off"];
/// Where the package keeps its own copy of the VC++ runtime, below [`VENDOR`].
const SOURCE_DIR: &str = "codex-resources/voice/bin";
/// The ripgrep.
const RG: &str = "rg.exe";
/// The VC++ runtime DLLs this step places, lowercase. Any other runtime DLL `rg.exe` imports is
/// reported as [`Outcome::Unsupported`].
const PLACED_DLLS: [&str; 2] = ["vcruntime140.dll", "vcruntime140_1.dll"];
/// The VC++ runtime's major version since Visual Studio 2015.
const RUNTIME_MAJOR: u16 = 14;
/// The largest file read as a PE (rg.exe is 3.9 MB, the DLL 0.2 MB).
const MAX_PE: u64 = 64 * 1024 * 1024;
/// The tracing target host.log keeps INFO from.
const LIFECYCLE: &str = "clauseo_agents::lifecycle";

/// Who signed a file, as Windows' Authenticode check found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Signer {
    /// A valid signature by Microsoft Corporation, chained to a Microsoft root.
    Microsoft,
    /// A valid signature by someone else (the organisation, and whether the root is Microsoft's).
    Other(String),
    /// No signature `WinVerifyTrust` accepts (its result, or why it was not asked).
    NotTrusted(String),
}

/// The facts about a file only Windows can read: its signer and its version resource.
pub(crate) trait Inspect {
    /// The file's Authenticode signer.
    fn signer(&self, path: &Path) -> Signer;
    /// The file's version resource (`FILEVERSION`), major first.
    fn file_version(&self, path: &Path) -> Option<[u16; 4]>;
}

/// The OS's checks: `WinVerifyTrust` and `GetFileVersionInfoW` on Windows. Elsewhere nothing
/// is trusted (this step never runs there outside tests).
struct System;

#[cfg(windows)]
impl Inspect for System {
    fn signer(&self, path: &Path) -> Signer {
        windows::signer(path)
    }

    fn file_version(&self, path: &Path) -> Option<[u16; 4]> {
        windows::file_version(path)
    }
}

#[cfg(not(windows))]
impl Inspect for System {
    fn signer(&self, _path: &Path) -> Signer {
        Signer::NotTrusted("Authenticode is checked on Windows only".to_owned())
    }

    fn file_version(&self, _path: &Path) -> Option<[u16; 4]> {
        None
    }
}

/// Why a source DLL was not placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refused {
    /// The package has no such file.
    Missing,
    /// It is not a PE file (the reader's reason) or did not read.
    NotPe(String),
    /// Its machine is not ARM64.
    WrongArch(u16),
    /// No signature Windows accepts.
    NotTrusted(String),
    /// Signed, but not by Microsoft.
    NotMicrosoft(String),
    /// It has no version resource.
    NoVersion,
    /// Its version is not a 14.x VC++ runtime.
    NotRuntime([u16; 4]),
    /// Older than the toolset `rg.exe` was linked with (found, needed major and minor).
    TooOld([u16; 4], (u16, u16)),
    /// `rg.exe`'s linker is not a 14.x MSVC (its version), so no runtime version can be judged.
    UnknownToolset((u8, u8)),
}

/// What happened to one `rg.exe` folder, or to one DLL in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// `rg.exe` imports no VC++ runtime DLL.
    NotNeeded,
    /// `rg.exe` did not read as a PE file.
    RgUnreadable(String),
    /// A file of the DLL's name is beside `rg.exe` already: left as it is.
    AlreadyBeside(String),
    /// The DLL was copied beside `rg.exe` (its name and version).
    Placed(String, [u16; 4]),
    /// The DLL was not placed (its name and why).
    Refused(String, Refused),
    /// `rg.exe` imports a runtime DLL this step does not place (its name).
    Unsupported(String),
    /// A file operation failed (the DLL's name and the error).
    Failed(String, String),
}

impl Outcome {
    /// Whether its log line warns: `rg.exe` will not start, or may not.
    fn warns(&self) -> bool {
        !matches!(
            self,
            Outcome::NotNeeded | Outcome::AlreadyBeside(_) | Outcome::Placed(..)
        )
    }
}

/// Places the VC++ runtime DLLs Codex's ARM64 `rg.exe` imports beside it in `folder` (an install
/// folder, or the temp folder about to become one), when the install is for Windows ARM64, and
/// logs what it did. Never fails.
pub(crate) fn place(config: &AgentsConfig, folder: &Path, agent: &AgentId) {
    for (dir, outcome) in place_with(config.platform, folder, &System) {
        let line = describe(&outcome);
        // The folder's name only (`codex-path` or `codex-path.sac-off`): a full path would put
        // the account's name in host.log.
        let dir = dir.file_name().unwrap_or_default().to_string_lossy();
        if outcome.warns() {
            tracing::warn!(target: LIFECYCLE, %agent, %dir, "agent install: {line}");
        } else {
            tracing::info!(target: LIFECYCLE, %agent, %dir, "agent install: {line}");
        }
    }
}

/// [`place`]'s work, with `inspect` for the checks only Windows can make: each outcome with the
/// folder holding `rg.exe`. Empty off Windows ARM64, and when no folder holds `rg.exe`.
pub(crate) fn place_with(
    platform: Platform,
    folder: &Path,
    inspect: &impl Inspect,
) -> Vec<(PathBuf, Outcome)> {
    if (platform.os, platform.arch) != (Os::Win32, Arch::Arm64) {
        return Vec::new();
    }
    let vendor = rel(folder, VENDOR);
    let source = rel(&vendor, SOURCE_DIR);
    RG_DIRS
        .iter()
        .map(|dir| vendor.join(dir))
        .filter(|dir| dir.join(RG).is_file())
        .flat_map(|dir| {
            place_in(&dir, &source, inspect)
                .into_iter()
                .map(move |outcome| (dir.clone(), outcome))
        })
        .collect()
}

/// One folder holding `rg.exe`: each runtime DLL it imports that is not beside it, placed from
/// `source` or refused.
fn place_in(dir: &Path, source: &Path, inspect: &impl Inspect) -> Vec<Outcome> {
    let rg = match read_pe(&dir.join(RG)) {
        Ok(rg) => rg,
        Err(err) => return vec![Outcome::RgUnreadable(err)],
    };
    let wanted: Vec<String> = runtime_imports(&rg);
    if wanted.is_empty() {
        return vec![Outcome::NotNeeded];
    }
    wanted
        .into_iter()
        .map(|dll| {
            if beside(dir, &dll) {
                Outcome::AlreadyBeside(dll)
            } else if !PLACED_DLLS.contains(&dll.as_str()) {
                Outcome::Unsupported(dll)
            } else {
                place_one(dir, source, &dll, rg.linker, inspect)
            }
        })
        .collect()
}

/// Copies `source/<dll>` beside `rg.exe` in `dir` under a temporary name, checks the copy, and
/// renames it to `dll` when it passes; removes it otherwise.
fn place_one(
    dir: &Path,
    source: &Path,
    dll: &str,
    linker: (u8, u8),
    inspect: &impl Inspect,
) -> Outcome {
    let from = source.join(dll);
    if !from.is_file() {
        return Outcome::Refused(dll.to_owned(), Refused::Missing);
    }
    // Ends in `.dll`, as Windows' version and signature calls expect; no loader looks for it.
    let tmp = dir.join(format!("clauseo-tmp.{dll}"));
    let to = dir.join(dll);
    let failed =
        |what: &str, err: std::io::Error| Outcome::Failed(dll.to_owned(), format!("{what}: {err}"));
    if let Err(err) = write::remove_file(&tmp) {
        return failed("clearing the temporary copy", err);
    }
    if let Err(err) = write::copy_file(&from, &tmp) {
        drop(write::remove_file(&tmp));
        return failed("copying", err);
    }
    let checked = read_pe(&tmp).map_err(Refused::NotPe).and_then(|info| {
        check(
            &info,
            &inspect.signer(&tmp),
            inspect.file_version(&tmp),
            linker,
        )
    });
    match checked {
        Ok(version) => match write::rename(&tmp, &to) {
            Ok(()) => Outcome::Placed(dll.to_owned(), version),
            Err(err) => {
                drop(write::remove_file(&tmp));
                failed("renaming the copy", err)
            }
        },
        Err(refused) => {
            if let Err(err) = write::remove_file(&tmp) {
                return failed("removing a refused copy", err);
            }
            Outcome::Refused(dll.to_owned(), refused)
        }
    }
}

/// The source checks, in order: ARM64, signed by Microsoft, a 14.x runtime no older than
/// `linker` (`rg.exe`'s linker version). The version when all pass.
pub(crate) fn check(
    info: &pe::PeInfo,
    signer: &Signer,
    version: Option<[u16; 4]>,
    linker: (u8, u8),
) -> Result<[u16; 4], Refused> {
    if info.machine != pe::MACHINE_ARM64 {
        return Err(Refused::WrongArch(info.machine));
    }
    match signer {
        Signer::Microsoft => {}
        Signer::Other(who) => return Err(Refused::NotMicrosoft(who.clone())),
        Signer::NotTrusted(why) => return Err(Refused::NotTrusted(why.clone())),
    }
    let version = version.ok_or(Refused::NoVersion)?;
    let needed = (u16::from(linker.0), u16::from(linker.1));
    if needed.0 != RUNTIME_MAJOR {
        return Err(Refused::UnknownToolset(linker));
    }
    let [major, minor, ..] = version;
    if major != RUNTIME_MAJOR {
        return Err(Refused::NotRuntime(version));
    }
    if (major, minor) < needed {
        return Err(Refused::TooOld(version, needed));
    }
    Ok(version)
}

/// Whether the C runtime rules forbid a DLL as an import of a file meant for a clean Windows: the
/// VC++ runtime family, which needs the Redistributable or an app-local copy. Mirrors
/// `CRT_RULES` in `.github/scripts/pe-crt-imports.mjs` (stage F): the Universal CRT
/// (`api-ms-win-crt-*`, `ucrtbase.dll`) and `msvcrt.dll` are the OS's and allowed; each VC++
/// stem wants a digit after it, so `msvcp_win.dll` and `mfcore.dll` pass.
pub(crate) fn is_forbidden(dll: &str) -> bool {
    let name = dll.to_ascii_lowercase();
    if name == "ucrtbased.dll" {
        return true;
    }
    let digit_after = |stem: &str| {
        name.strip_prefix(stem)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_ascii_digit())
    };
    [
        "vcruntime",
        "msvcp",
        "msvcr",
        "vccorlib",
        "concrt",
        "vcomp",
        "vcamp",
        "mfc",
        "mfcm",
    ]
    .into_iter()
    .any(digit_after)
}

/// The forbidden ([`is_forbidden`]) DLLs `rg` imports or delay-imports, lowercase, each once.
fn runtime_imports(rg: &pe::PeInfo) -> Vec<String> {
    let mut dlls: Vec<String> = rg
        .all_imports()
        .filter(|dll| is_forbidden(dll))
        .map(str::to_ascii_lowercase)
        .collect();
    dlls.sort();
    dlls.dedup();
    dlls
}

/// The forbidden DLLs the `rg.exe` in `dir` imports with no file of that name beside it: what
/// stage F's `pe-crt-imports.mjs --app-local-ok` would fail on. Err when `rg.exe` does not read.
/// Tests only: the install reports per DLL instead (`Outcome`).
#[cfg(test)]
pub(crate) fn missing_beside(dir: &Path) -> Result<Vec<String>, String> {
    let rg = read_pe(&dir.join(RG))?;
    Ok(runtime_imports(&rg)
        .into_iter()
        .filter(|dll| !beside(dir, dll))
        .collect())
}

/// Whether a file named `dll` (any case, as Windows' loader matches) is in `dir`.
fn beside(dir: &Path, dll: &str) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(dll)
                && entry.file_type().is_ok_and(|kind| kind.is_file())
        })
    })
}

/// Reads and parses the PE file at `path`, through no link.
fn read_pe(path: &Path) -> Result<pe::PeInfo, String> {
    use std::io::Read as _;
    let file = write::open_read(path).map_err(|err| err.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_PE)
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    pe::parse(&bytes).map_err(|err| err.to_string())
}

/// `dir` (`/`-separated) below `folder`.
fn rel(folder: &Path, dir: &str) -> PathBuf {
    dir.split('/')
        .fold(folder.to_path_buf(), |path, part| path.join(part))
}

/// A version as `a.b.c.d`.
fn dotted(version: [u16; 4]) -> String {
    let [a, b, c, d] = version;
    format!("{a}.{b}.{c}.{d}")
}

/// `outcome`'s log line.
fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::NotNeeded => "Codex's rg.exe imports no VC++ runtime DLL".to_owned(),
        Outcome::RgUnreadable(err) => {
            format!("Codex's rg.exe did not read, so no VC++ runtime was placed: {err}")
        }
        Outcome::AlreadyBeside(dll) => format!("{dll} is beside Codex's rg.exe already"),
        Outcome::Placed(dll, version) => format!(
            "{dll} {} (Microsoft-signed, ARM64) copied beside Codex's rg.exe",
            dotted(*version)
        ),
        Outcome::Refused(dll, refused) => format!(
            "{dll} not placed beside Codex's rg.exe, which may not start without it: {}",
            refused_text(refused)
        ),
        Outcome::Unsupported(dll) => {
            format!("Codex's rg.exe imports {dll}, which Clauseo does not place; it may not start")
        }
        Outcome::Failed(dll, detail) => format!("{dll} beside Codex's rg.exe: {detail}"),
    }
}

/// Why, in words.
fn refused_text(refused: &Refused) -> String {
    match refused {
        Refused::Missing => format!("the package has no copy in {SOURCE_DIR}"),
        Refused::NotPe(err) => format!("the package's copy is not a PE file ({err})"),
        Refused::WrongArch(machine) => {
            format!("the package's copy is not ARM64 (machine 0x{machine:04X})")
        }
        Refused::NotTrusted(why) => format!("the package's copy has no valid signature ({why})"),
        Refused::NotMicrosoft(who) => {
            format!("the package's copy is not signed by Microsoft ({who})")
        }
        Refused::NoVersion => "the package's copy has no version resource".to_owned(),
        Refused::NotRuntime(version) => format!(
            "the package's copy is version {}, not a VC++ 14.x runtime",
            dotted(*version)
        ),
        Refused::TooOld(version, (major, minor)) => format!(
            "the package's copy is version {}, older than rg.exe's toolset {major}.{minor}",
            dotted(*version)
        ),
        Refused::UnknownToolset((major, minor)) => {
            format!("rg.exe's linker version {major}.{minor} is not a VC++ 14.x toolset")
        }
    }
}

#[cfg(test)]
mod tests;
