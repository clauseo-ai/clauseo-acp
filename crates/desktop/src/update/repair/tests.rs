//! The repair's tests: the plan (the tag from the build's mode, eligibility, the URLs and the
//! key), the marker, the sweep, the flow end to end against a release served over HTTPS
//! (`flow.rs`), and, on Windows only, the attempt's handle and folder (`windows.rs`).
#![expect(
    clippy::disallowed_methods,
    reason = "test set-up writes and removes files in its scratch folder; no write module here"
)]

use clauseo_host::InstallKind;
use clauseo_protocol::{Timestamp, UpdateState};

use super::plan::{self, Inputs, NotEligible, Plan, tag};
use crate::commands::{ReleaseMode, build_release_mode, release_mode};
use crate::test_support::Scratch;

mod flow;
mod marker;
mod sweep;
#[cfg(windows)]
mod windows;

/// The version every test runs as.
const VERSION: &str = "0.4.1";

/// A per-user Windows x64 test build of [`VERSION`] with `key`, no update under way.
fn inputs(key: &str) -> Inputs<'_> {
    Inputs {
        os: "windows",
        target_arch: "x86_64",
        kind: InstallKind::CurrentUser,
        mode: ReleaseMode::Test,
        version: VERSION,
        pubkey: Some(key),
        update: None,
    }
}

/// The tag is the build's release mode's, never read from the version: `v` for a release,
/// `test-v` for a test build, none for a dry run. The mode is compiled in: unset, as here, an
/// `e2e` build is a test build and any other a release.
#[test]
fn the_tag_comes_from_the_builds_mode() {
    assert_eq!(
        tag(ReleaseMode::Release, VERSION).as_deref(),
        Some("v0.4.1")
    );
    assert_eq!(
        tag(ReleaseMode::Test, VERSION).as_deref(),
        Some("test-v0.4.1")
    );
    assert_eq!(tag(ReleaseMode::Dry, VERSION), None);
    assert_eq!(release_mode(None, true), ReleaseMode::Test);
    assert_eq!(release_mode(None, false), ReleaseMode::Release);
    assert_eq!(release_mode(Some("release"), true), ReleaseMode::Release);
    assert_eq!(release_mode(Some("dry"), false), ReleaseMode::Dry);
    let this_build = if cfg!(feature = "e2e") {
        "test-v0.4.1"
    } else {
        "v0.4.1"
    };
    assert_eq!(
        tag(build_release_mode(), VERSION).as_deref(),
        Some(this_build)
    );
}

/// The plan names this version's per-user installer for the running architecture, on the
/// mode's tag, its `.sig` beside it, and the key.
#[test]
fn the_plan_names_this_versions_installer_for_this_architecture() {
    let planned = plan::plan(inputs(" key ")).unwrap();
    assert_eq!(
        planned,
        Plan {
            version: VERSION.to_owned(),
            file: "Clauseo_0.4.1_x64-setup.exe".to_owned(),
            installer: "https://github.com/clauseo-ai/clauseo-acp/releases/download/test-v0.4.1/Clauseo_0.4.1_x64-setup.exe"
                .parse()
                .unwrap(),
            signature: "https://github.com/clauseo-ai/clauseo-acp/releases/download/test-v0.4.1/Clauseo_0.4.1_x64-setup.exe.sig"
                .parse()
                .unwrap(),
            pubkey: "key".to_owned(),
        }
    );
    let arm = plan::plan(Inputs {
        target_arch: "aarch64",
        mode: ReleaseMode::Release,
        ..inputs("key")
    })
    .unwrap();
    assert_eq!(arm.file, "Clauseo_0.4.1_arm64-setup.exe");
    assert_eq!(
        arm.installer.as_str(),
        "https://github.com/clauseo-ai/clauseo-acp/releases/download/v0.4.1/Clauseo_0.4.1_arm64-setup.exe"
    );
}

/// Per machine, a read-only location and a package are not eligible, nor anything but Windows,
/// a dry run, a build with no key, a version that is not plain, an architecture with no
/// installer, or an update scheduled or installing. A download under way or a failed check is
/// no install.
#[test]
fn only_a_per_user_windows_install_with_a_key_is_eligible() {
    let refused = |inputs: Inputs<'_>| plan::plan(inputs).err();
    for kind in [
        InstallKind::PerMachine,
        InstallKind::ReadOnlyLocation,
        InstallKind::Package,
    ] {
        assert_eq!(
            refused(Inputs {
                kind,
                ..inputs("key")
            }),
            Some(NotEligible("not a per-user install")),
            "{kind:?}"
        );
    }
    assert!(
        refused(Inputs {
            os: "macos",
            ..inputs("key")
        })
        .is_some()
    );
    assert!(
        refused(Inputs {
            mode: ReleaseMode::Dry,
            ..inputs("key")
        })
        .is_some()
    );
    assert!(
        refused(Inputs {
            pubkey: None,
            ..inputs("key")
        })
        .is_some()
    );
    assert!(refused(inputs("  ")).is_some());
    assert!(
        refused(Inputs {
            version: "0.4.1-beta",
            ..inputs("key")
        })
        .is_some()
    );
    assert!(
        refused(Inputs {
            target_arch: "x86",
            ..inputs("key")
        })
        .is_some()
    );
    let installing = UpdateState::Installing {
        version: "0.4.2".to_owned(),
    };
    let scheduled = UpdateState::Scheduled {
        version: "0.4.2".to_owned(),
    };
    for state in [&installing, &scheduled] {
        assert_eq!(
            refused(Inputs {
                update: Some(state),
                ..inputs("key")
            }),
            Some(NotEligible("an update is installing")),
        );
    }
    // A firm's policy that turned updates off (or anyone else managing them).
    for by in [
        clauseo_protocol::ManagedBy::Policy,
        clauseo_protocol::ManagedBy::PerMachine,
    ] {
        assert_eq!(
            refused(Inputs {
                update: Some(&UpdateState::Managed { by }),
                ..inputs("key")
            }),
            Some(NotEligible("updates are managed")),
            "{by:?}"
        );
    }
    let downloading = UpdateState::Downloading {
        version: "0.4.2".to_owned(),
        received: clauseo_protocol::Count::ZERO,
        total: None,
    };
    let failed = UpdateState::Failed {
        version: None,
        at: Timestamp::ZERO,
        stage: clauseo_protocol::FailedStage::Check,
    };
    for state in [&UpdateState::Idle, &downloading, &failed] {
        assert!(
            plan::plan(Inputs {
                update: Some(state),
                ..inputs("key")
            })
            .is_ok()
        );
    }
}

/// The key is the build's mode's: the config's for a release (none while it is empty, as
/// today), the test key for a test build (the `e2e` build alone has it), none for a dry run.
#[test]
fn the_key_is_the_builds_modes() {
    let config = &crate::context::<tauri::test::MockRuntime>()
        .config()
        .clone();
    assert_eq!(
        super::key_for(ReleaseMode::Release, config),
        super::super::plugin::Source::Config.pubkey(config)
    );
    assert_eq!(
        super::key_for(ReleaseMode::Test, config),
        super::super::choice::TEST_KEY.map(|key| key.trim().to_owned())
    );
    assert_eq!(super::key_for(ReleaseMode::Dry, config), None);
}

/// A start with a fresh marker for this version tells the window it came back from the
/// installer, and deletes the marker; a start without one says nothing.
#[test]
fn the_start_reads_the_marker_into_the_note() {
    let scratch = Scratch::new("repair-state", &[]);
    let locations = clauseo_settings::Locations::under_root(scratch.path());
    std::fs::create_dir_all(locations.data_dir()).unwrap();
    let marker = locations.data_dir().join(super::MARKER);
    let state = super::RepairState::at_start(&locations, VERSION);
    assert_eq!(state.note(), None);
    super::marker::write(&marker, VERSION, super::now_secs()).unwrap();
    let state = super::RepairState::at_start(&locations, VERSION);
    assert_eq!(state.note(), Some(super::RepairNote::Returned));
    assert!(!marker.exists());
    assert_eq!(
        super::RepairState::at_start(&locations, VERSION).note(),
        None
    );
}

/// `display_info`'s `repair_available` follows the updater's state when the host has answered:
/// managed (a firm policy), scheduled or installing make it false; idle, a download or an
/// unknown state leave it to the build and install.
#[test]
fn the_offer_follows_the_updaters_state() {
    use clauseo_protocol::ManagedBy;
    for refusing in [
        UpdateState::Managed {
            by: ManagedBy::Policy,
        },
        UpdateState::Scheduled {
            version: "0.4.2".to_owned(),
        },
        UpdateState::Installing {
            version: "0.4.2".to_owned(),
        },
    ] {
        assert!(super::update_refuses(Some(&refusing)), "{refusing:?}");
    }
    assert!(!super::update_refuses(Some(&UpdateState::Idle)));
    assert!(!super::update_refuses(None));
}

/// `lib.rs` holds an exit request with no code (the last window closing) only while the repair
/// hands over; the repair's own quit, and any other with a code, goes on.
#[test]
fn the_exit_is_held_only_while_the_windows_are_handed_over() {
    use std::sync::atomic::Ordering;
    assert!(!super::holds_exit(None));
    super::HANDING_OVER.store(true, Ordering::Release);
    assert!(super::holds_exit(None));
    assert!(!super::holds_exit(Some(0)));
    super::HANDING_OVER.store(false, Ordering::Release);
    assert!(!super::holds_exit(None));
}

/// A repair's end clears the running flag (so Restart to update passes again) after a cancel, a
/// refusal or a failure; only a started installer keeps it, as the app quits.
#[test]
fn restart_to_update_is_unblocked_unless_the_installer_started() {
    use super::flow::Refusal;
    use super::{RepairEnd, finish, running, set_running};
    set_running(true);
    assert_eq!(finish(Ok(RepairEnd::Cancelled)), Ok(RepairEnd::Cancelled));
    assert!(!running());
    set_running(true);
    assert!(finish(Err(Refusal::DownloadFailed)).is_err());
    assert!(!running());
    set_running(true);
    assert_eq!(finish(Ok(RepairEnd::Started)), Ok(RepairEnd::Started));
    assert!(running());
    set_running(false);
}

/// Only a missing admin copy is the installer's to repair: no hash in the record (or a record
/// that can't be read, as the launch check reads it), or no file at its place.
#[test]
fn the_repair_runs_only_while_the_admin_copy_is_missing() {
    use super::plan::copy_missing;
    assert!(!copy_missing(Ok(true), true));
    assert!(copy_missing(Ok(false), true));
    assert!(copy_missing(Ok(true), false));
    assert!(copy_missing(Err(()), true));
    assert!(copy_missing(Ok(false), false));
}

/// The installer this branch runs must handle `/CLAUSEO_REPAIR` (stage F's installer change:
/// wait for Clauseo to be gone, then start it again once at the end). Fails until that change is
/// in `crates/desktop/windows/`: the repair must merge after stage F's installer PR.
#[test]
fn the_installer_handles_the_repair_switch() {
    let switch = super::flow::ARGS[0];
    assert_eq!(switch, "/CLAUSEO_REPAIR");
    let windows = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows");
    let mut found = Vec::new();
    let mut folders = vec![windows];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "nsh" || ext == "nsi")
                && std::fs::read_to_string(&path).is_ok_and(|text| text.contains(switch))
            {
                found.push(path);
            }
        }
    }
    assert!(
        !found.is_empty(),
        "no NSIS hook under crates/desktop/windows handles {switch}: stage F's installer change \
         (installer-repair-switch) must land first"
    );
}

/// A sealed attempt's folder must list its installer alone (`.` and `..` aside): another file,
/// a stream's name, a folder, a second entry or none refuses.
#[test]
fn the_sealed_folder_holds_its_installer_alone() {
    use super::plan::only_installer;
    let names = |list: &[&str]| {
        list.iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    let exe = "Clauseo_0.4.1_x64-setup.exe";
    assert!(only_installer(&names(&[".", "..", exe]), exe));
    assert!(only_installer(&names(&[exe]), exe));
    for refused in [
        names(&[".", "..", exe, "version.dll"]),
        names(&["version.dll", exe]),
        names(&[".", "..", "version.dll"]),
        names(&[".", ".."]),
        names(&[]),
        names(&[exe, exe]),
        names(&[&format!("{exe}:payload")]),
        names(&[exe, "sub"]),
    ] {
        assert!(!only_installer(&refused, exe), "{refused:?}");
    }
}
