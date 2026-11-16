//! The VC++ runtime DLL Codex's ARM64 `rg.exe` imports, placed beside it (Windows ARM64 only).
//!
//! `@openai/codex`'s `win32-arm64` package ships the official ripgrep 15.2.0
//! `aarch64-pc-windows-msvc` `rg.exe` in `vendor/aarch64-pc-windows-msvc/codex-path/`. Unlike
//! the x64 build, it links the VC++ runtime dynamically (ripgrep's `.cargo/config.toml` sets
//! `+crt-static` for `x86_64` only) and imports `VCRUNTIME140.dll`, which a clean Windows 11 ARM64
//! install does not have: `rg` fails to start (0xC0000135) and Codex's search with it. `rg.exe`
//! is never changed or rebuilt (Smart App Control pins its bytes, and an unsigned rebuild has no
//! reputation); instead the runtime DLLs it imports are copied beside it, the app-local
//! deployment Microsoft supports.
//!
//! The source is the copy the same package already ships for Codex's voice host,
//! `vendor/aarch64-pc-windows-msvc/codex-resources/voice/bin/vcruntime140.dll` (14.50.35719.0
//! in 0.162.1). It is copied (never hard-linked: a Codex update replaces the source) to a
//! temporary name beside `rg.exe`, and the copy is checked there, so no other file can be
//! swapped in between the check and the move to its real name, which never replaces a file:
//! - an ARM64 PE (machine `0xAA64`);
//! - an Authenticode signature `WinVerifyTrust` (`WINTRUST_ACTION_GENERIC_VERIFY_V2`) accepts,
//!   whose signer certificate's common name and organisation are both `Microsoft Corporation`
//!   (Microsoft's third-party driver-signing certificates carry the organisation but another
//!   name) and whose chain ends at a Microsoft root (`CERT_CHAIN_POLICY_MICROSOFT_ROOT`, flight
//!   roots refused); a hash pin alone would not say who made it;
//! - a version resource whose `OriginalFilename` is the DLL's own name (any case), so another
//!   Microsoft DLL renamed is refused;
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
mod text;
#[cfg(windows)]
mod windows;

use text::describe;

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
/// The largest file read as a PE (rg.exe is 3.9 MB, the DLL 0.2 MB); a larger one is refused.
const MAX_PE: u64 = 64 * 1024 * 1024;
/// The tracing target host.log keeps INFO from.
const LIFECYCLE: &str = "clauseo_agents::lifecycle";

/// Who signed a file, as Windows' Authenticode check found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Signer {
    /// A valid signature whose signer certificate names Microsoft Corporation as both its
    /// common name and its organisation, chained to a Microsoft root.
    Microsoft,
    /// A valid signature by someone else (the organisation, and whether the root is Microsoft's).
    Other(String),
    /// No signature `WinVerifyTrust` accepts (its result, or why it was not asked).
    NotTrusted(String),
}

/// The common name and organisation Microsoft's own code-signing certificates carry.
/// Its third-party driver-signing certificates (`CN=Microsoft Windows Hardware Compatibility
/// Publisher`) carry the organisation alone, so both are compared.
const MICROSOFT: &str = "Microsoft Corporation";

/// The signer a valid signature stands for, from its signer certificate's subject common name
/// and organisation and whether its chain ends at a Microsoft root: [`Signer::Microsoft`] only
/// when both names are exactly Microsoft Corporation and the root is Microsoft's.
pub(crate) fn judge(common_name: &str, organisation: &str, microsoft_root: bool) -> Signer {
    if common_name == MICROSOFT && organisation == MICROSOFT && microsoft_root {
        Signer::Microsoft
    } else {
        Signer::Other(format!(
            "common name {common_name:?}, organisation {organisation:?}, Microsoft root: \
             {microsoft_root}"
        ))
    }
}

/// What a file's version resource says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VersionInfo {
    /// `FILEVERSION`, major first.
    pub(crate) version: [u16; 4],
    /// The `OriginalFilename` string, when the resource has one.
    pub(crate) original_filename: Option<String>,
}

/// The facts about a file only Windows can read: its signer and its version resource.
pub(crate) trait Inspect {
    /// The file's Authenticode signer.
    fn signer(&self, path: &Path) -> Signer;
    /// The file's version resource.
    fn version_info(&self, path: &Path) -> Option<VersionInfo>;
}

/// The OS's checks: `WinVerifyTrust` and `GetFileVersionInfoW` on Windows. Elsewhere nothing
/// is trusted (this step never runs there outside tests).
struct System;

#[cfg(windows)]
impl Inspect for System {
    fn signer(&self, path: &Path) -> Signer {
        windows::signer(path)
    }

    fn version_info(&self, path: &Path) -> Option<VersionInfo> {
        windows::version_info(path)
    }
}

#[cfg(not(windows))]
impl Inspect for System {
    fn signer(&self, _path: &Path) -> Signer {
        Signer::NotTrusted("Authenticode is checked on Windows only".to_owned())
    }

    fn version_info(&self, _path: &Path) -> Option<VersionInfo> {
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
    /// Its `OriginalFilename` is not the DLL's name (what it is, if any): another DLL renamed.
    WrongName(Option<String>),
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
            inspect.version_info(&tmp).as_ref(),
            dll,
            linker,
        )
    });
    match checked {
        // A hard link to the checked copy, then the copy's name removed: unlike a rename, the
        // link fails when a file of that name appeared since the look above, never replacing it.
        Ok(version) => match write::hard_link(&tmp, &to) {
            Ok(()) => {
                if let Err(err) = write::remove_file(&tmp) {
                    return failed("removing the temporary name", err);
                }
                Outcome::Placed(dll.to_owned(), version)
            }
            Err(err) => {
                drop(write::remove_file(&tmp));
                failed("moving the copy into place", err)
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

/// The source checks, in order: ARM64, signed by Microsoft, a version resource whose
/// `OriginalFilename` is `dll` (any case), a 14.x runtime no older than `linker` (`rg.exe`'s
/// linker version). The version when all pass.
pub(crate) fn check(
    info: &pe::PeInfo,
    signer: &Signer,
    resource: Option<&VersionInfo>,
    dll: &str,
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
    let resource = resource.ok_or(Refused::NoVersion)?;
    let named = resource.original_filename.as_deref();
    if !named.is_some_and(|name| name.trim().eq_ignore_ascii_case(dll)) {
        return Err(Refused::WrongName(named.map(str::to_owned)));
    }
    let version = resource.version;
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

/// Reads and parses the PE file at `path`, through no link; a file over [`MAX_PE`] is refused.
fn read_pe(path: &Path) -> Result<pe::PeInfo, String> {
    read_pe_within(path, MAX_PE)
}

/// [`read_pe`] with a cap of `limit` bytes.
fn read_pe_within(path: &Path, limit: u64) -> Result<pe::PeInfo, String> {
    use std::io::Read as _;
    let file = write::open_read(path).map_err(|err| err.to_string())?;
    let mut bytes = Vec::new();
    // One byte past the cap tells a larger file from one exactly at it: refused, never parsed cut.
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(format!("larger than {limit} bytes"));
    }
    pe::parse(&bytes).map_err(|err| err.to_string())
}

/// `dir` (`/`-separated) below `folder`.
fn rel(folder: &Path, dir: &str) -> PathBuf {
    dir.split('/')
        .fold(folder.to_path_buf(), |path, part| path.join(part))
}

#[cfg(test)]
mod tests;
