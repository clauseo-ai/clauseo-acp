//! The MSI's checks, over a fake file system: each failing check is `Failed`, with no path, and
//! msiexec is never run; the held handle outlives the install.

use std::cell::RefCell;
use std::rc::Rc;

use super::{
    InstallError, LOCAL_SYSTEM, MsiFiles, MsiUpgrade, Signature, TRUSTED_INSTALLER, upgrade_with,
};
use crate::windows::wsl::FakeWsl;

const APPS: &str = r"C:\Program Files\WindowsApps";
/// A Microsoft code-signing signature.
const SIGNED: Signature = Signature {
    microsoft_root: true,
    code_signing: true,
};
const PACKAGE: &str = "MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3.0_x64__8wekyb3d8bbwe";

/// What the fake answers, and what happened, in order.
struct Fake {
    open: Result<(), String>,
    owner: Result<&'static str, String>,
    signed: Result<Signature, String>,
    events: Rc<RefCell<Vec<String>>>,
}

impl Fake {
    fn passing() -> Fake {
        Fake {
            open: Ok(()),
            owner: Ok(TRUSTED_INSTALLER),
            signed: Ok(SIGNED),
            events: Rc::default(),
        }
    }

    fn log(&self, event: String) {
        self.events.borrow_mut().push(event);
    }
}

/// The held handle: logs its close.
struct Held(Rc<RefCell<Vec<String>>>);

impl Drop for Held {
    fn drop(&mut self) {
        self.0.borrow_mut().push("closed".to_owned());
    }
}

impl MsiFiles for Fake {
    type Held = Held;

    fn open(&self, apps: &str, package: &str) -> Result<Held, String> {
        self.log(format!("open {apps} {package}"));
        self.open.clone().map(|()| Held(Rc::clone(&self.events)))
    }

    fn owner(&self, _msi: &Held) -> Result<String, String> {
        self.log("owner".to_owned());
        self.owner.clone().map(str::to_owned)
    }

    fn signature(&self, _msi: &Held, path: &str) -> Result<Signature, String> {
        self.log(format!("signer {path}"));
        self.signed.clone()
    }
}

/// The upgrade with `files`, an MSI at 2.6.1 and the image in `PACKAGE`'s folder; the install
/// logs its path and succeeds.
fn run(files: &Fake) -> MsiUpgrade {
    let reg = FakeWsl {
        msi_version: Some("2.6.1.0".to_owned()),
        ..FakeWsl::default()
    };
    let image = format!(r#""{APPS}\{PACKAGE}\wslinstaller.exe""#);
    upgrade_with(files, &reg, &image, APPS, |path, _held| {
        files.log(format!("install {path}"));
        Ok(())
    })
}

fn msi_path() -> String {
    format!(r"{APPS}\{PACKAGE}\wsl.msi")
}

/// `files`' upgrade failed, without a path in its reason, and msiexec never ran.
fn assert_refused(files: &Fake) -> String {
    let MsiUpgrade::Failed(why) = run(files) else {
        panic!("not refused");
    };
    assert!(!why.contains('\\') && !why.contains(PACKAGE), "{why}");
    let events = files.events.borrow();
    assert!(
        !events.iter().any(|event| event.starts_with("install")),
        "{events:?}"
    );
    why
}

#[test]
fn a_verified_msi_is_installed_by_its_verified_path_and_held_until_msiexec_ends() {
    for owner in [TRUSTED_INSTALLER, LOCAL_SYSTEM] {
        let files = Fake {
            owner: Ok(owner),
            ..Fake::passing()
        };
        let upgraded = MsiUpgrade::Upgraded {
            from: "2.6.1.0".to_owned(),
            to: (2, 6, 3),
        };
        assert_eq!(run(&files), upgraded);
        let path = msi_path();
        let expected = [
            format!("open {APPS} {PACKAGE}"),
            "owner".to_owned(),
            format!("signer {path}"),
            format!("install {path}"),
            "closed".to_owned(),
        ];
        assert_eq!(*files.events.borrow(), expected);
    }
}

#[test]
fn a_reparse_point_or_a_linked_file_is_refused() {
    let files = Fake {
        open: Err("wsl.msi: a reparse point (tag 0xA0000003)".to_owned()),
        ..Fake::passing()
    };
    assert!(assert_refused(&files).contains("reparse point"));
    assert_eq!(
        files.events.borrow().len(),
        1,
        "nothing after the refused open"
    );
    let files = Fake {
        open: Err("wsl.msi has 2 links".to_owned()),
        ..Fake::passing()
    };
    assert_refused(&files);
}

#[test]
fn an_msi_owned_by_anyone_else_is_refused_before_its_signature() {
    let administrators = "S-1-5-32-544";
    let lawyer = "S-1-5-21-1-2-3-1001";
    for owner in [administrators, lawyer, "", "s-1-5-18 "] {
        let files = Fake {
            owner: Ok(owner),
            ..Fake::passing()
        };
        assert!(assert_refused(&files).contains("owner"), "{owner}");
        let events = files.events.borrow();
        assert!(
            !events.iter().any(|event| event.starts_with("signer")),
            "{events:?}"
        );
        assert_eq!(events.last().map(String::as_str), Some("closed"));
    }
    let files = Fake {
        owner: Err("GetSecurityInfo failed: 5".to_owned()),
        ..Fake::passing()
    };
    assert_refused(&files);
}

#[test]
fn an_untrusted_signature_or_a_chain_to_another_root_is_refused() {
    let files = Fake {
        signed: Err("WinVerifyTrust refused wsl.msi: 0x800B0100".to_owned()),
        ..Fake::passing()
    };
    assert!(assert_refused(&files).contains("WinVerifyTrust"));
    // Trusted, but its chain ends in a root that isn't Microsoft's.
    let files = Fake {
        signed: Ok(Signature {
            microsoft_root: false,
            ..SIGNED
        }),
        ..Fake::passing()
    };
    assert!(assert_refused(&files).contains("Microsoft root"));
    // A Microsoft chain whose leaf isn't for code signing.
    let files = Fake {
        signed: Ok(Signature {
            code_signing: false,
            ..SIGNED
        }),
        ..Fake::passing()
    };
    assert!(assert_refused(&files).contains("code-signing"));
}

#[test]
fn nothing_is_opened_when_no_upgrade_is_owed_or_the_folder_isn_t_wsl_s() {
    let files = Fake::passing();
    let reg = FakeWsl {
        msi_version: Some("2.6.3.0".to_owned()),
        ..FakeWsl::default()
    };
    let image = format!(r"{APPS}\{PACKAGE}\wslinstaller.exe");
    let never = |_: &str, _: &Held| -> Result<(), InstallError> { panic!("installed") };
    assert_eq!(
        upgrade_with(&files, &reg, &image, APPS, never),
        MsiUpgrade::NotPending
    );
    let other = PACKAGE.replace("8wekyb3d8bbwe", "0000000000000");
    let image = format!(r"{APPS}\{other}\wslinstaller.exe");
    let refused = upgrade_with(&files, &reg, &image, APPS, never);
    assert!(matches!(refused, MsiUpgrade::Failed(_)), "{refused:?}");
    assert!(files.events.borrow().is_empty());
}

#[test]
fn a_failed_install_is_failed_and_the_handle_is_still_closed_after_it() {
    let files = Fake::passing();
    let reg = FakeWsl {
        msi_version: Some("2.6.1.0".to_owned()),
        ..FakeWsl::default()
    };
    let image = format!(r"{APPS}\{PACKAGE}\wslinstaller.exe");
    let upgraded = upgrade_with(&files, &reg, &image, APPS, |_, _| {
        files.log("install".to_owned());
        Err(InstallError::Failed("msiexec ended with 1603".to_owned()))
    });
    assert_eq!(
        upgraded,
        MsiUpgrade::Failed("msiexec ended with 1603".to_owned())
    );
    assert_eq!(
        files.events.borrow().last().map(String::as_str),
        Some("closed")
    );
}

#[test]
fn msiexec_still_running_at_its_limit_is_its_own_outcome() {
    let files = Fake::passing();
    let reg = FakeWsl {
        msi_version: Some("2.6.1.0".to_owned()),
        ..FakeWsl::default()
    };
    let image = format!(r"{APPS}\{PACKAGE}\wslinstaller.exe");
    let why = "msiexec didn't end within 240 s; it was left running";
    let upgraded = upgrade_with(&files, &reg, &image, APPS, |_, _| {
        Err(InstallError::StillInstalling(why.to_owned()))
    });
    assert_eq!(upgraded, MsiUpgrade::StillInstalling(why.to_owned()));
}
