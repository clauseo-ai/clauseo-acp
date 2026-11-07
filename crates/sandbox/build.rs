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
//! accounts and profiles), and since S15 PR7 `fwpuclnt.dll` (the WFP filters) and
//! `iphlpapi.dll` (the port check's listener table). They load at their first call, after `main`
//! has run `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)`, so they and their own
//! imports come from System32; nothing calls them before `main` (no static initialiser or TLS
//! callback in the crate). `tests/setup_exe.rs` checks the exe's import and delay-import
//! directories, its version information and its one manifest.
//!
//! The runner exe (`clauseo-sandbox-run`, S15 `PR5`) runs as the user, so it gets no manifest,
//! but it sits in the same folder and starts the agent under another account: it gets the same
//! flag, and `userenv.dll` (`GetUserProfileDirectoryW`) delay-loaded, its one direct import
//! that isn't vetted in `tests/setup_exe.rs`. It reaches no `Net*` call, so it doesn't import
//! `netapi32.dll`; `/DELAYLOAD` of a DLL it doesn't import would only warn (LNK4199).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

type Error = Box<dyn std::error::Error + Send + Sync>;

const BIN: &str = "clauseo-sandbox-setup";

fn main() -> Result<(), Error> {
    println!("cargo:rerun-if-changed=build.rs");
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
            "delayimp.lib",
            "/Brepro",
            "/DEBUG:NONE",
        ] {
            println!("cargo:rustc-link-arg-bin={BIN}={arg}");
        }
        let dir = std::env::var("CARGO_MANIFEST_DIR")?;
        let manifest = Path::new(&dir).join("setup.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!(
            "cargo:rustc-link-arg-bin={BIN}=/MANIFESTINPUT:{}",
            manifest.display()
        );
        version_info(Path::new(&dir))?;
        for arg in [
            "/DEPENDENTLOADFLAG:0x800",
            "/DELAYLOAD:userenv.dll",
            "delayimp.lib",
        ] {
            println!("cargo:rustc-link-arg-bin=clauseo-sandbox-run={arg}");
        }
    }
    Ok(())
}

/// Writes the setup exe's `VERSIONINFO` `.rc` into `OUT_DIR` and links it into that exe.
fn version_info(dir: &Path) -> Result<(), Error> {
    let conf = dir.join("../desktop/tauri.conf.json");
    println!("cargo:rerun-if-changed={}", conf.display());
    let conf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&conf)?)?;
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
