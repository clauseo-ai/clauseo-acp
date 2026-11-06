//! The setup exe's manifest (S15 `PR4h`, D-B14): on `*-windows-msvc` the linker embeds one that
//! asks for an administrator, for that binary only, so Windows raises the UAC prompt when the
//! app or the uninstaller starts it. `asInvoker` never. No new dependency and no `.rc` file;
//! other targets get nothing. Its static imports load from System32 only
//! (`/DEPENDENTLOADFLAG:0x800`, `LOAD_LIBRARY_SEARCH_SYSTEM32`), so a DLL planted beside the
//! exe in the install folder is never loaded into the elevated process.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.ends_with("-windows-msvc") {
        for arg in [
            "/MANIFEST:EMBED",
            "/MANIFESTUAC:level='requireAdministrator' uiAccess='false'",
            "/DEPENDENTLOADFLAG:0x800",
        ] {
            println!("cargo:rustc-link-arg-bin=clauseo-sandbox-setup={arg}");
        }
    }
}
