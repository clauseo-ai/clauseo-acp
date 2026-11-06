use super::SystemEnv;
use crate::locations::Env;
use std::ffi::OsString;

#[test]
fn system_env_reads_the_process_environment() {
    // The test runner sets this for every test process.
    assert_eq!(
        SystemEnv.var("CARGO_MANIFEST_DIR"),
        Some(OsString::from(env!("CARGO_MANIFEST_DIR")))
    );
    assert_eq!(SystemEnv.var("CLAUSEO_SETTINGS_TEST_UNSET_VARIABLE"), None);
}

/// The planted home of [`windows_without_documents`]'s child.
#[cfg(windows)]
const PLANTED_HOME: &str = "CLAUSEO_SETTINGS_TEST_PLANTED_HOME";

/// Issue #365 through the real `SHGetKnownFolderPath`: Windows stores Documents as
/// `%USERPROFILE%\Documents` and expands it with the process's environment, so a child whose
/// planted USERPROFILE has no Documents gets none (the trigger of #351's canary, job
/// 126628848298). `CLAUSEO_DATA_DIR` at a drive root has no sibling, so the default matters
/// folder needs Documents, and `Locations::resolve`, the call the desktop's setup makes, must
/// still succeed, with no default matters folder.
#[cfg(windows)]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "makes and removes its own planted home and reruns this test binary in it; nothing \
              else is launched or written"
)]
fn windows_without_documents() {
    use crate::locations::{KnownFolder, PlatformDirs as _};
    use crate::system::SystemDirs;
    // Documents moved off the profile (Known Folder Move): the planted USERPROFILE would not
    // reach it, so there is nothing to show here.
    let profile = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from);
    let documents = SystemDirs.known_folder(KnownFolder::Documents);
    let under_profile = profile
        .zip(documents)
        .is_some_and(|(profile, documents)| documents.starts_with(profile));
    if !under_profile {
        eprintln!("Documents is not under USERPROFILE here; skipped");
        return;
    }
    let home = std::env::temp_dir().join(format!("clauseo-settings-365-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "system::tests::windows_without_documents_child",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env("USERPROFILE", &home)
        .env(PLANTED_HOME, &home)
        .env("CLAUSEO_DATA_DIR", r"C:\")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    std::fs::remove_dir_all(&home).unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("1 passed"), "the child ran: {text}");
}

/// [`windows_without_documents`]'s child: run only by it, with the planted home.
#[cfg(windows)]
#[test]
#[ignore = "run by windows_without_documents with a planted USERPROFILE"]
fn windows_without_documents_child() {
    use crate::locations::{KnownFolder, Locations, PlatformDirs as _};
    use crate::system::{SystemDirs, SystemEnv};
    let home = std::path::PathBuf::from(std::env::var_os(PLANTED_HOME).unwrap());
    assert!(!home.join("Documents").exists());
    assert_eq!(SystemDirs.known_folder(KnownFolder::Documents), None);
    let at = Locations::resolve(&SystemDirs, &SystemEnv).unwrap();
    assert_eq!(at.data_dir(), std::path::Path::new(r"C:\"));
    assert_eq!(at.default_matters_dir(), None);
}
