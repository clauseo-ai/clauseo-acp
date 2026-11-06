//! The setup exe as built (S15 `PR4h`). On Windows (msvc) its embedded manifest asks for an
//! administrator (D-B14): the linker writes the manifest's XML into the exe's `RT_MANIFEST`
//! resource as it is, so the exe's bytes hold it. Off Windows the exe exits `Usage` (1) with
//! one line. Its PE load configuration keeps static imports to System32
//! (`DependentLoadFlags` = `LOAD_LIBRARY_SEARCH_SYSTEM32`, `/DEPENDENTLOADFLAG:0x800`).

// Used by the msvc tests and the off-Windows one; windows-gnu builds neither.
#[cfg(any(not(windows), target_env = "msvc"))]
const EXE: &str = env!("CARGO_BIN_EXE_clauseo-sandbox-setup");

#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn the_embedded_manifest_requires_an_administrator() {
    let bytes = std::fs::read(EXE).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    let at = text
        .find("requestedExecutionLevel")
        .expect("the exe embeds a manifest with a requested execution level");
    let element: String = text[at..].chars().take(120).collect();
    assert!(
        element.contains("level='requireAdministrator'")
            || element.contains("level=\"requireAdministrator\""),
        "{element}"
    );
    assert!(
        element.contains("uiAccess='false'") || element.contains("uiAccess=\"false\""),
        "{element}"
    );
    assert!(
        !text.contains("asInvoker"),
        "the manifest never asks to run as the invoker"
    );
}

/// `bytes[at..at + N]`; `None` past the end.
#[cfg(all(windows, target_env = "msvc"))]
fn le<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at + N).and_then(|part| part.try_into().ok())
}

#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn static_imports_load_from_system32_only() {
    let pe = std::fs::read(EXE).unwrap();
    let u16_at = |at: usize| usize::from(u16::from_le_bytes(le::<2>(&pe, at).unwrap()));
    let u32_at =
        |at: usize| usize::try_from(u32::from_le_bytes(le::<4>(&pe, at).unwrap())).unwrap();
    let header = u32_at(0x3C);
    assert_eq!(le::<4>(&pe, header), Some(*b"PE\0\0"));
    let sections = u16_at(header + 6);
    let optional = header + 24;
    let optional_size = u16_at(header + 20);
    assert_eq!(u16_at(optional), 0x20B, "a PE32+ image");
    // The data directories start 112 bytes into a PE32+ optional header; LOAD_CONFIG is the
    // eleventh (index 10), eight bytes each.
    let load_config_rva = u32_at(optional + 112 + 10 * 8);
    assert_ne!(load_config_rva, 0, "the exe has a load configuration");
    // The RVA to a file offset, through the section that holds it.
    let table = optional + optional_size;
    let offset = (0..sections)
        .map(|n| table + n * 40)
        .find_map(|at| {
            let (virtual_size, rva) = (u32_at(at + 8), u32_at(at + 12));
            let raw = u32_at(at + 20);
            (rva..rva + virtual_size)
                .contains(&load_config_rva)
                .then(|| load_config_rva - rva + raw)
        })
        .expect("a section holds the load configuration");
    // IMAGE_LOAD_CONFIG_DIRECTORY64: its Size first, DependentLoadFlags (a u16) at 0x4E.
    assert!(
        u32_at(offset) > 0x4E + 2,
        "the load configuration reaches DependentLoadFlags"
    );
    assert_eq!(u16_at(offset + 0x4E), 0x800, "LOAD_LIBRARY_SEARCH_SYSTEM32");
}

#[cfg(not(windows))]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test runs the built exe itself"
)]
fn off_windows_the_exe_exits_usage_with_one_line() {
    let output = std::process::Command::new(EXE)
        .arg("--set-up")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.contains("Windows"), "{stderr}");
}
