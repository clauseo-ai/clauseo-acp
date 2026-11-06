//! The setup exe's manifest (S15 `PR4h`, D-B14): on `*-windows-msvc` the linker embeds one that
//! asks for an administrator, for that binary only, so Windows raises the UAC prompt when the
//! app or the uninstaller starts it. `asInvoker` never. No new dependency and no `.rc` file;
//! other targets get nothing.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.ends_with("-windows-msvc") {
        for arg in [
            "/MANIFEST:EMBED",
            "/MANIFESTUAC:level='requireAdministrator' uiAccess='false'",
        ] {
            println!("cargo:rustc-link-arg-bin=clauseo-sandbox-setup={arg}");
        }
    }
}
