//! The setup exe's manifest (S15 `PR4h`, D-B14): on `*-windows-msvc` the linker embeds one that
//! asks for an administrator, for that binary only, so Windows raises the UAC prompt when the
//! app or the uninstaller starts it. `asInvoker` never. The linker merges `setup.manifest` (the
//! Windows versions it supports, the app's list, `/MANIFESTINPUT`) into that one manifest; no
//! `.rc` file holds it. Other targets get nothing.
//!
//! The build is reproducible at the release's build path (the repo's `target/`): rebuilding the
//! release commit there gives the same exe, so a hash Microsoft has cleared stays cleared.
//! `/Brepro` puts a hash of the image where the link time would go, and in a `REPRO` entry of the
//! debug directory. `/DEBUG:NONE`, after the `/DEBUG` rustc always passes on msvc, writes no PDB
//! and no `CODEVIEW` entry naming one: lld's PDB GUID is a hash of the PDB, and the PDB holds the
//! paths of rustc's randomly named temp dir (`deps/rustcXXXXXX/symbols.o`, the `.natvis` files),
//! so two builds at the same path gave two GUIDs, two `/Brepro` stamps and nothing else
//! different (CI run 42362946949). So this exe ships without a `.pdb`. A different target dir
//! would still give a different exe: RUSTFLAGS' remaps (`.github/scripts/sidecars.mjs`) miss the
//! host's proc macros and build scripts under `--target`. `.github/workflows/setup-exe-repro.yml`
//! builds the exe twice, from clean, in that one target dir, and compares them.
//!
//! The setup exe's version information, on `*-windows-msvc` and for that binary only: a
//! `VERSIONINFO` resource written into `OUT_DIR` as `.rc` text and compiled by `embed-resource`
//! (already in the tree through tauri-build). The exe is unsigned, and Defender's ML model
//! flagged it with no metadata; it now carries what the app's own exe carries (tauri-build's
//! resource): `ProductName` from `productName`, `CompanyName` from `bundle.publisher`,
//! `LegalCopyright` from `bundle.copyright` and both versions from `version`, all read from
//! `crates/desktop/tauri.conf.json` (the release checks that its version is
//! `crates/desktop/Cargo.toml`'s). The `.rc` holds no manifest: the linker's stays the only one.
//!
//! A DLL planted beside the exe in the install folder must not load into the elevated process
//! (ADR 0049). Its direct static imports load from System32 only (`/DEPENDENTLOADFLAG:0x800`).
//! That flag doesn't cover what those imports import in turn, which the loader resolves before
//! `main` with the exe's folder first: `netapi32.dll` pulls in `netutils.dll` and `samcli.dll`,
//! which aren't `KnownDLLs`. So the imports whose own imports aren't all `KnownDLLs` are
//! delay-loaded (`/DELAYLOAD`, with `delayimp.lib`): `netapi32.dll` and `userenv.dll` (the
//! accounts and profiles), since S15 PR7 `fwpuclnt.dll` (the WFP filters) and `iphlpapi.dll`
//! (the port check's listener table), `wtsapi32.dll` (the stop's process list,
//! `WTSEnumerateProcessesExW`), and `wintrust.dll` (the WSL MSI's signature check). They load
//! at their first call, after `main` has run
//! `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)`, so they and their own imports come
//! from System32; nothing calls them before `main` (no static initialiser or TLS callback in the
//! crate). `tests/setup_exe.rs` checks the exe's import and delay-import directories, its
//! version information and its one manifest.
//!
//! On every target it embeds the updater key the admin copy's refresh trusts (ADR 0049, "Admin
//! copy refresh"; `src/windows/refresh/key.rs`) as `CLAUSEO_SANDBOX_UPDATER_KEY`. The key is
//! `CLAUSEO_UPDATER_PUBKEY` when set, else `plugins.updater.pubkey` from `tauri.conf.json`,
//! both Tauri's base64 of a minisign public-key file; with neither (a dry build), the placeholder,
//! which verifies nothing, so such a build's refresh fails closed. A key that isn't a minisign
//! public key fails the build, and so does a release build (`CLAUSEO_RELEASE_MODE=release`, any
//! profile) whose key isn't the pinned production key, `PRODUCTION_KEY_ID`, or while none is
//! pinned (`release_guard`). release.yml's build legs export `CLAUSEO_RELEASE_MODE` as their mode
//! (`release`, `test` or `dry`); any other value, the empty string included, fails the build, so
//! a typo can't turn the guard off. On msvc the exe's `VERSIONINFO` also carries the key's id as
//! the string `UpdaterKeyId`, which release.yml's assemble job checks from the file without
//! running it.
//!
//! The runner exe (`clauseo-sandbox-run`, S15 `PR5`) runs as the user, so it gets no manifest,
//! but it sits in the same folder and starts the agent under another account: it gets the same
//! flag, and `userenv.dll` (`GetUserProfileDirectoryW`) and `wtsapi32.dll`
//! (`WTSEnumerateProcessesW`, the sweep's) delay-loaded, its direct imports that aren't vetted in
//! `tests/setup_exe.rs` (neither is a `KnownDLL`). It reaches no
//! `Net*` call, so it doesn't import `netapi32.dll`; `/DELAYLOAD` of a DLL it doesn't import
//! would only warn (LNK4199).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

// The crate compiles this file too, as the public `windows::refresh::key`.
#[path = "src/windows/refresh/key.rs"]
#[expect(
    unreachable_pub,
    reason = "public in the crate, which compiles the same file"
)]
mod key;

type Error = Box<dyn std::error::Error + Send + Sync>;

const BIN: &str = "clauseo-sandbox-setup";

fn main() -> Result<(), Error> {
    println!("cargo:rerun-if-changed=build.rs");
    // Every target: the app's version, which `--adopt` records as `AdminCopyVersion`
    // (`windows::admin_copy::APP_VERSION`), the same string as the exe's `FileVersion`.
    let dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let version = tauri_conf(Path::new(&dir))?
        .pointer("/version")
        .and_then(serde_json::Value::as_str)
        .ok_or("tauri.conf.json has no string at /version")?
        .to_owned();
    println!("cargo:rustc-env=CLAUSEO_APP_VERSION={version}");
    let key_id = refresh_key(Path::new(&dir))?;
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.ends_with("-windows-msvc") {
        for arg in [
            "/MANIFEST:EMBED",
            "/MANIFESTUAC:level='requireAdministrator' uiAccess='false'",
            "/DEPENDENTLOADFLAG:0x800",
            "/DELAYLOAD:netapi32.dll",
            "/DELAYLOAD:userenv.dll",
            "/DELAYLOAD:fwpuclnt.dll",
            "/DELAYLOAD:iphlpapi.dll",
            "/DELAYLOAD:wtsapi32.dll",
            "/DELAYLOAD:wintrust.dll",
            "delayimp.lib",
            "/Brepro",
            "/DEBUG:NONE",
        ] {
            println!("cargo:rustc-link-arg-bin={BIN}={arg}");
        }
        let manifest = Path::new(&dir).join("setup.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!(
            "cargo:rustc-link-arg-bin={BIN}=/MANIFESTINPUT:{}",
            manifest.display()
        );
        version_info(Path::new(&dir), key_id)?;
        for arg in [
            "/DEPENDENTLOADFLAG:0x800",
            "/DELAYLOAD:userenv.dll",
            "/DELAYLOAD:wtsapi32.dll",
            "delayimp.lib",
        ] {
            println!("cargo:rustc-link-arg-bin=clauseo-sandbox-run={arg}");
        }
    }
    Ok(())
}

/// Embeds the refresh's updater key, as the module says, and returns its id.
fn refresh_key(dir: &Path) -> Result<u64, Error> {
    println!("cargo:rerun-if-changed=src/windows/refresh/key.rs");
    println!("cargo:rerun-if-env-changed=CLAUSEO_UPDATER_PUBKEY");
    println!("cargo:rerun-if-env-changed=CLAUSEO_RELEASE_MODE");
    let conf = tauri_conf(dir)?;
    let from_env = std::env::var("CLAUSEO_UPDATER_PUBKEY").unwrap_or_default();
    let from_conf = conf
        .pointer("/plugins/updater/pubkey")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let given = [from_env.as_str(), from_conf]
        .into_iter()
        .map(key::compact)
        .find(|candidate| !candidate.is_empty());
    let chosen = given.unwrap_or_else(|| key::compact(key::PLACEHOLDER_KEY));
    let id = key::key_id(&chosen).map_err(|why| format!("the updater public key: {why}"))?;
    let mode = std::env::var("CLAUSEO_RELEASE_MODE").ok();
    key::release_guard(mode.as_deref(), id)?;
    println!("cargo:rustc-env=CLAUSEO_SANDBOX_UPDATER_KEY={chosen}");
    Ok(id)
}

/// `crates/desktop/tauri.conf.json`, parsed; cargo reruns the script when it changes.
fn tauri_conf(dir: &Path) -> Result<serde_json::Value, Error> {
    let conf = dir.join("../desktop/tauri.conf.json");
    println!("cargo:rerun-if-changed={}", conf.display());
    Ok(serde_json::from_str(&std::fs::read_to_string(&conf)?)?)
}

/// Writes the setup exe's `VERSIONINFO` `.rc` into `OUT_DIR` and links it into that exe.
fn version_info(dir: &Path, key_id: u64) -> Result<(), Error> {
    let conf = tauri_conf(dir)?;
    let field = |pointer: &str| -> Result<String, Error> {
        let value = conf.pointer(pointer).and_then(serde_json::Value::as_str);
        Ok(value
            .ok_or(format!("tauri.conf.json has no string at {pointer}"))?
            .to_owned())
    };
    let version = field("/version")?;
    let strings = [
        ("CompanyName", field("/bundle/publisher")?),
        ("FileDescription", "Clauseo sandbox set-up".to_owned()),
        ("FileVersion", version.clone()),
        ("InternalName", format!("{BIN}.exe")),
        ("LegalCopyright", field("/bundle/copyright")?),
        ("OriginalFilename", format!("{BIN}.exe")),
        ("ProductName", field("/productName")?),
        ("ProductVersion", version.clone()),
        (key::KEY_ID_VERSION_STRING, format!("{key_id:016X}")),
    ];
    let rc = PathBuf::from(std::env::var("OUT_DIR")?).join("setup-version.rc");
    write_out(&rc, &rc_text(&numeric(&version)?, &strings)?)?;
    embed_resource::compile_for(&rc, [BIN], embed_resource::NONE)
        .manifest_required()
        .map_err(|failed| format!("the setup exe's VERSIONINFO: {failed}"))?;
    Ok(())
}

/// Writes the build script's own `.rc` into `OUT_DIR`.
#[expect(
    clippy::disallowed_methods,
    reason = "build script: writes its generated .rc into OUT_DIR, cargo's folder for it"
)]
fn write_out(path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)
}

/// `1.2.3-beta+4` as `1,2,3,0`: the numeric version's four 16-bit parts.
fn numeric(version: &str) -> Result<String, Error> {
    let core = version.split(['-', '+']).next().unwrap_or_default();
    let mut parts = core
        .split('.')
        .map(str::parse::<u16>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("version {version} isn't numeric: {error}"))?;
    parts.resize(4, 0);
    Ok(parts
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(","))
}

/// The `.rc` text: one `VERSIONINFO` (ID 1), US English and Unicode (`040904B0`), no manifest.
/// Numbers instead of `winver.h`'s names, so it compiles without the preprocessor's includes:
/// `FILEOS` `VOS_NT_WINDOWS32` (0x40004), `FILETYPE` `VFT_APP` (1).
fn rc_text(numeric: &str, strings: &[(&str, String)]) -> Result<String, Error> {
    let mut values = String::new();
    for (name, value) in strings {
        let value = value.replace('"', "\"\"");
        writeln!(values, "      VALUE \"{name}\", \"{value}\"")?;
    }
    Ok(format!(
        "1 VERSIONINFO\nFILEVERSION {numeric}\nPRODUCTVERSION {numeric}\nFILEFLAGSMASK 0x3F\n\
         FILEFLAGS 0\nFILEOS 0x40004\nFILETYPE 1\nFILESUBTYPE 0\nBEGIN\n  \
         BLOCK \"StringFileInfo\"\n  BEGIN\n    BLOCK \"040904B0\"\n    BEGIN\n{values}    END\n  \
         END\n  BLOCK \"VarFileInfo\"\n  BEGIN\n    VALUE \"Translation\", 0x409, 1200\n  END\nEND\n"
    ))
}
