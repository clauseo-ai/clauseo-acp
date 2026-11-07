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

/// The planted home of [`windows_roaming_unchecked`]'s child.
#[cfg(windows)]
const ROAMING_HOME: &str = "CLAUSEO_SETTINGS_TEST_ROAMING_HOME";

/// The roaming app data folder is looked up without Windows' check, which touches the network
/// when a firm redirects it to a share that may be down: a child whose planted USERPROFILE has no
/// `AppData\Roaming` still gets the folder's path, for the refused folders.
#[cfg(windows)]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "makes and removes its own planted home and reruns this test binary in it; nothing \
              else is launched or written"
)]
fn windows_roaming_unchecked() {
    use crate::locations::{KnownFolder, PlatformDirs as _};
    use crate::system::SystemDirs;
    let profile = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from);
    let roaming = SystemDirs.known_folder(KnownFolder::RoamingAppData);
    let under_profile = profile
        .zip(roaming)
        .is_some_and(|(profile, roaming)| roaming.starts_with(profile));
    if !under_profile {
        eprintln!("Roaming AppData is not under USERPROFILE here; skipped");
        return;
    }
    let home = std::env::temp_dir().join(format!("clauseo-settings-roam-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "system::tests::windows_roaming_unchecked_child",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env("USERPROFILE", &home)
        .env(ROAMING_HOME, &home)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    std::fs::remove_dir_all(&home).unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("1 passed"), "the child ran: {text}");
}

/// [`windows_roaming_unchecked`]'s child: run only by it, with the planted home.
#[cfg(windows)]
#[test]
#[ignore = "run by windows_roaming_unchecked with a planted USERPROFILE"]
fn windows_roaming_unchecked_child() {
    use crate::locations::{KnownFolder, PlatformDirs as _};
    use crate::system::SystemDirs;
    let home = std::path::PathBuf::from(std::env::var_os(ROAMING_HOME).unwrap());
    let roaming = home.join("AppData").join("Roaming");
    assert!(!roaming.exists());
    assert_eq!(
        SystemDirs.known_folder(KnownFolder::RoamingAppData),
        Some(roaming)
    );
}

/// The planted share of [`windows_documents_on_a_dead_share`]'s child.
#[cfg(windows)]
const DEAD_SHARE: &str = r"\\192.0.2.1\clauseo-test";

/// A Documents folder on a share that does not answer, through the real `SystemDirs`: a child
/// whose planted USERPROFILE is a share at a TEST-NET-1 address (RFC 5737, never routed) gets
/// Documents as `<share>\Documents`, as [`windows_without_documents`] explains, and must get
/// `None` within the 2 s wait. Windows' own check of the folder (`KF_FLAG_DEFAULT`) waits on
/// SMB for about 20 s there, so going back to it fails the child's time limit. A network that
/// refuses the address at once instead of dropping it gives a fast `None` either way: the test
/// then passes but proves less.
#[cfg(windows)]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "reruns this test binary with a planted USERPROFILE; nothing else is launched or \
              written"
)]
fn windows_documents_on_a_dead_share() {
    use crate::locations::{KnownFolder, PlatformDirs as _};
    use crate::system::SystemDirs;
    let profile = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from);
    let documents = SystemDirs.known_folder(KnownFolder::Documents);
    let under_profile = profile
        .zip(documents)
        .is_some_and(|(profile, documents)| documents.starts_with(profile));
    if !under_profile {
        eprintln!("Documents is not under USERPROFILE here; skipped");
        return;
    }
    // The child may take longer to exit than to pass: its check is left stuck on the share.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "system::tests::windows_documents_on_a_dead_share_child",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env("USERPROFILE", DEAD_SHARE)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("1 passed"), "the child ran: {text}");
}

/// [`windows_documents_on_a_dead_share`]'s child: run only by it, with the planted share.
#[cfg(windows)]
#[test]
#[ignore = "run by windows_documents_on_a_dead_share with a planted USERPROFILE"]
#[expect(
    clippy::disallowed_methods,
    reason = "checks a bound on wall-clock time"
)]
fn windows_documents_on_a_dead_share_child() {
    use crate::locations::{KnownFolder, PlatformDirs as _};
    use crate::system::SystemDirs;
    assert_eq!(
        std::env::var_os("USERPROFILE"),
        Some(std::ffi::OsString::from(DEAD_SHARE))
    );
    let started = std::time::Instant::now();
    let documents = SystemDirs.known_folder(KnownFolder::Documents);
    let took = started.elapsed();
    assert_eq!(documents, None);
    assert!(took < std::time::Duration::from_secs(5), "{took:?}");
}
