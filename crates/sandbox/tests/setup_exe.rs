//! The setup exe as built (S15 `PR4h`). On Windows (msvc) its embedded manifest asks for an
//! administrator (D-B14): the linker writes the manifest's XML into the exe's `RT_MANIFEST`
//! resource as it is, so the exe's bytes hold it. Off Windows the exe exits `Usage` (1) with
//! one line.

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
