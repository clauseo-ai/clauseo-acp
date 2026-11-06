//! The setup exe's manifest (S15 `PR4h`, D-B14): on `*-windows-msvc` the linker embeds one that
//! asks for an administrator, for that binary only, so Windows raises the UAC prompt when the
//! app or the uninstaller starts it. `asInvoker` never. No new dependency and no `.rc` file;
//! other targets get nothing.
//!
//! A DLL planted beside the exe in the install folder must not load into the elevated process
//! (ADR 0049). Its direct static imports load from System32 only (`/DEPENDENTLOADFLAG:0x800`).
//! That flag doesn't cover what those imports import in turn, which the loader resolves before
//! `main` with the exe's folder first: `netapi32.dll` pulls in `netutils.dll` and `samcli.dll`,
//! which aren't `KnownDLLs`. So `netapi32.dll` and `userenv.dll`, the imports whose own imports
//! aren't all `KnownDLLs`, are delay-loaded (`/DELAYLOAD`, with `delayimp.lib`): they load at
//! their first call, after `main` has run `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)`,
//! so they and their own imports come from System32. `tests/setup_exe.rs` checks the exe's
//! import and delay-import directories.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.ends_with("-windows-msvc") {
        for arg in [
            "/MANIFEST:EMBED",
            "/MANIFESTUAC:level='requireAdministrator' uiAccess='false'",
            "/DEPENDENTLOADFLAG:0x800",
            "/DELAYLOAD:netapi32.dll",
            "/DELAYLOAD:userenv.dll",
            "delayimp.lib",
        ] {
            println!("cargo:rustc-link-arg-bin=clauseo-sandbox-setup={arg}");
        }
    }
}
