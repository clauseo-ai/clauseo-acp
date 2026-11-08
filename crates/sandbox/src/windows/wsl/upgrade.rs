//! A pending WSL upgrade, as WSL's own installer judges it (`WslInstaller.cpp:142-165`,
//! `IsUpdateNeeded`, at microsoft/WSL @19cd15ea): the Store package's `WslInstaller` service
//! upgrades through the MSI when `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Lxss\MSI`'s
//! `Version` is empty or older than the package's own version (major, minor, revision). Any
//! user may start that upgrade: the package's packaged COM class `WslInstaller` grants launch to
//! Authenticated Users (`AppxManifest.in:74-77`), and the MSI rewrites WSL's launch permissions,
//! dropping the deny until the WSL deny task puts it back. So a tier B launch doesn't start while
//! one is pending ([`Blocked::WslUpgradePending`]): it has the WSL deny task install it first,
//! then re-apply the deny (`clean_close::wsl_heal`, `msi.rs`), and starts once the deny is
//! verified.
//!
//! The package's version is read from its packaged-COM registration,
//! `HKLM\SOFTWARE\Classes\PackagedCom\Package\<full name>`, which `Users` can read: the full name
//! is `MicrosoftCorporationII.WindowsSubsystemForLinux_<major>.<minor>.<revision>.0_...`,
//! and the newest listed counts.
//!
//! A package registered for one user only may not be listed there: its packaged-COM
//! registration is that user's, in `HKCU\Software\Classes\PackagedCom\Package`. So with none in
//! `HKLM`, the lawyer's own (the check runs as the lawyer) is read before taking it as
//! [`Upgrade::NoPackage`]. A package registered only for the lawyer is still compared, its
//! version taken from the lawyer's registration: newer than the MSI is [`Upgrade::Pending`], as
//! new or older is [`Upgrade::LawyerOnly`], and a version that can't be read or parsed is
//! [`Upgrade::LawyerUnreadable`]. A pool user's hive holds no registration of it, so a pool user
//! shouldn't be able to activate `WslInstaller`'s class, but that isn't verified, and a pending
//! upgrade is installed before the launch whoever could start it.
//!
//! [`Blocked::WslUpgradePending`]: crate::Blocked::WslUpgradePending

use std::time::Duration;

use super::seam::WslRead;

/// How long the run waits for msiexec: an upgrade takes tens of seconds; the task's limit is 5
/// minutes (`rules::WSL_DENY_EXECUTION_LIMIT`), and the deny is re-applied after the wait.
/// The launch's wait for the upgrade task's run follows it (`clean_close::wsl_heal`).
pub const MSI_WAIT: Duration = Duration::from_secs(240);

/// The package's name, the start of each of its full names.
pub const PACKAGE_NAME: &str = "MicrosoftCorporationII.WindowsSubsystemForLinux";

/// A version as WSL compares them: major, minor, revision.
pub type Version = (u32, u32, u32);

/// The publisher hash of the package's family, `<PACKAGE_NAME>_8wekyb3d8bbwe`: Microsoft's.
pub const PACKAGE_PUBLISHER: &str = "8wekyb3d8bbwe";
/// The architectures a package folder's name may name.
const ARCHITECTURES: [&str; 5] = ["x64", "arm64", "x86", "arm", "neutral"];

/// The packaged service whose image is in the package's folder (`AppxManifest.in:50`).
pub const INSTALLER_SERVICE: &str = "WslInstaller";
/// The service's image, in the package's folder.
const INSTALLER_EXE: &str = "wslinstaller.exe";
/// WSL's MSI, in the package's folder: `GetMsiPackagePath` (`WslInstaller.cpp:21-32`) joins
/// it to the folder of the service's own image (`GetBasePath`).
pub const PACKAGE_MSI: &str = "wsl.msi";

/// What [`upgrade`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upgrade {
    /// No WSL package is registered: nothing can upgrade through it.
    NoPackage,
    /// The WSL package is registered for the lawyer only (`HKCU`), and the MSI is as new as
    /// the lawyer's package, or newer.
    LawyerOnly,
    /// The installed MSI is as new as the package, or newer.
    Current,
    /// The MSI is older than the package: an upgrade is pending.
    Pending,
    /// The package is registered (machine-wide) and no MSI version is recorded (missing or
    /// empty): WSL's installer would install the MSI, not upgrade it, and nothing says when, so
    /// this isn't a pending upgrade but a state the check can't judge ([`check`]).
    ///
    /// [`check`]: super::check
    NoMsi,
    /// The package is registered (machine-wide) and a version can't be read or parsed, or the
    /// registrations can't be listed.
    Unreadable,
    /// The package is registered for the lawyer only and a version (the lawyer's package's or
    /// the MSI's) can't be read or parsed: unverifiable where WSL 2 can run ([`check`]).
    ///
    /// [`check`]: super::check
    LawyerUnreadable,
}

/// The first three parts of `text`, a dotted version of three or four numbers (the fourth,
/// the package's build part, is left out, as WSL leaves it out); `None` otherwise.
pub fn version(text: &str) -> Option<Version> {
    let parts: Vec<u32> = text
        .trim()
        .split('.')
        .map(|part| part.parse::<u32>().ok())
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [major, minor, revision] | [major, minor, revision, _] => Some((*major, *minor, *revision)),
        _ => None,
    }
}

/// The version in a package full name `<PACKAGE_NAME>_<version>_<arch>_<resource>_<publisher>`;
/// `None` when it isn't the package's or its version doesn't parse.
fn package_version(full_name: &str) -> Option<Version> {
    let rest = full_name.strip_prefix(PACKAGE_NAME)?.strip_prefix('_')?;
    version(rest.split('_').next()?)
}

/// The version in a WSL package folder's name, `<PACKAGE_NAME>_<version>_<arch>__<publisher>`
/// (no resource id): the name and [`PACKAGE_PUBLISHER`] compared exactly, ASCII case ignored,
/// the version four dotted numbers, the architecture a known one. `None` for any other name.
pub fn package_folder_version(name: &str) -> Option<Version> {
    let parts: Vec<&str> = name.split('_').collect();
    let [family, numbers, arch, "", publisher] = parts.as_slice() else {
        return None;
    };
    let digits = numbers
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.');
    let known = ARCHITECTURES
        .iter()
        .any(|known| known.eq_ignore_ascii_case(arch));
    if !family.eq_ignore_ascii_case(PACKAGE_NAME)
        || !publisher.eq_ignore_ascii_case(PACKAGE_PUBLISHER)
        || !known
        || !digits
        || numbers.split('.').count() != 4
    {
        return None;
    }
    version(numbers)
}

/// The `WslInstaller` service's package folder and the package's version, from the service's
/// `ImagePath` `image_path` (quoted or not): `None` unless the image is `wslinstaller.exe` in a
/// WSL package's folder ([`package_folder_version`]) directly under `windows_apps`
/// (`%ProgramFiles%\WindowsApps`, which only `TrustedInstaller` writes). WSL's own installer
/// runs the MSI in that folder ([`PACKAGE_MSI`]). Text only, so it is tested on every OS.
pub fn installer_package(image_path: &str, windows_apps: &str) -> Option<(String, Version)> {
    let text = image_path.trim();
    let exe = match text.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next()?,
        None => text,
    };
    let (folder, name) = exe.rsplit_once('\\')?;
    let (parent, package) = folder.rsplit_once('\\')?;
    let apps = windows_apps.trim_end_matches('\\');
    if !name.eq_ignore_ascii_case(INSTALLER_EXE) || !parent.eq_ignore_ascii_case(apps) {
        return None;
    }
    Some((folder.to_owned(), package_folder_version(package)?))
}

/// Whether the MSI version `msi` is older than `package`, as [`upgrade`] compares them for
/// [`Upgrade::Pending`]: a missing, empty or unparsed version isn't.
pub fn msi_behind(package: Version, msi: Option<&str>) -> bool {
    msi.and_then(version)
        .is_some_and(|installed| installed < package)
}

/// Whether a WSL upgrade is pending, as the module says. A listing that fails (the lawyer's
/// own registrations' too) is [`Upgrade::Unreadable`]; a listed full name whose version doesn't
/// parse, or an MSI version that can't be read or parsed, is [`Upgrade::Unreadable`] for a
/// machine-wide package and [`Upgrade::LawyerUnreadable`] for the lawyer's own; a missing or
/// empty MSI version is [`Upgrade::NoMsi`].
pub fn upgrade(reg: &dyn WslRead) -> Upgrade {
    let Ok(machine) = reg.wsl_packages() else {
        return Upgrade::Unreadable;
    };
    let lawyer = machine.is_empty();
    let names = if lawyer {
        match reg.user_wsl_packages() {
            Err(_) => return Upgrade::Unreadable,
            Ok(mine) => mine,
        }
    } else {
        machine
    };
    let (unreadable, current) = if lawyer {
        (Upgrade::LawyerUnreadable, Upgrade::LawyerOnly)
    } else {
        (Upgrade::Unreadable, Upgrade::Current)
    };
    let Some(versions) = names
        .iter()
        .map(|name| package_version(name))
        .collect::<Option<Vec<Version>>>()
    else {
        return unreadable;
    };
    let Some(package) = versions.into_iter().max() else {
        return Upgrade::NoPackage;
    };
    match reg.msi_version() {
        Err(_) => unreadable,
        Ok(None) => Upgrade::NoMsi,
        Ok(Some(text)) if text.trim().is_empty() => Upgrade::NoMsi,
        Ok(Some(text)) => match version(&text) {
            None => unreadable,
            Some(installed) if installed < package => Upgrade::Pending,
            Some(_) => current,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{installer_package, msi_behind, package_folder_version};

    const APPS: &str = r"C:\Program Files\WindowsApps";
    const FOLDER: &str = concat!(
        r"C:\Program Files\WindowsApps\",
        "MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3.0_x64__8wekyb3d8bbwe"
    );
    const OTHER: &str =
        "MicrosoftCorporationII.WindowsSubsystemForLinux_9.0.0.0_x64__8wekyb3d8bbwe";

    #[test]
    fn the_installer_service_s_folder_is_a_wsl_package_s_folder_under_windows_apps() {
        let quoted = format!(r#""{FOLDER}\wslinstaller.exe""#);
        let found = Some((FOLDER.to_owned(), (2, 6, 3)));
        assert_eq!(installer_package(&quoted, APPS), found);
        let bare = format!(r"{FOLDER}\WslInstaller.exe");
        assert_eq!(installer_package(&bare, &format!("{APPS}\\")), found);
        // Windows' paths are case-blind; the package's name is as the package spells it.
        let lower = bare.replace(APPS, &APPS.to_lowercase());
        assert!(installer_package(&lower, APPS).is_some());
        // Anywhere else, another image, another package or a deeper folder: none.
        let elsewhere = format!(r"C:\Temp\{OTHER}\wslinstaller.exe");
        let deeper = format!(r"{FOLDER}\sub\wslinstaller.exe");
        let other = format!(r"{APPS}\Contoso.Tool_9.0.0.0_x64__abc\wslinstaller.exe");
        let climbing = format!(r"{APPS}\..\{OTHER}\wslinstaller.exe");
        let for_image = |image: &str| installer_package(image, APPS);
        assert_eq!(for_image(&elsewhere), None);
        assert_eq!(for_image(&deeper), None);
        assert_eq!(for_image(&other), None);
        assert_eq!(for_image(&climbing), None);
        assert_eq!(for_image(&format!(r"{FOLDER}\wsl.exe")), None);
        assert_eq!(for_image(""), None);
    }

    #[test]
    fn a_package_folder_is_wsl_s_only_with_its_family_s_name_and_publisher() {
        let ok = "MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3.0_x64__8wekyb3d8bbwe";
        assert_eq!(package_folder_version(ok), Some((2, 6, 3)));
        let arm = "microsoftcorporationii.windowssubsystemforlinux_2.6.3.0_ARM64__8WEKYB3D8BBWE";
        assert_eq!(package_folder_version(arm), Some((2, 6, 3)));
        for name in [
            // Another publisher, a longer or shorter name, a resource id, an unknown arch.
            "MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3.0_x64__8wekyb3d8bbwf",
            "MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3.0_x64__8wekyb3d8bbwex",
            "MicrosoftCorporationII.WindowsSubsystemForLinuxX_2.6.3.0_x64__8wekyb3d8bbwe",
            "Contoso.MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3.0_x64__8wekyb3d8bbwe",
            "MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3.0_x64_en-us_8wekyb3d8bbwe",
            "MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3.0_mips__8wekyb3d8bbwe",
            // A version of three parts, with a space or a sign, or none; parts missing or added.
            "MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3_x64__8wekyb3d8bbwe",
            "MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3.0 _x64__8wekyb3d8bbwe",
            "MicrosoftCorporationII.WindowsSubsystemForLinux_+2.6.3.0_x64__8wekyb3d8bbwe",
            "MicrosoftCorporationII.WindowsSubsystemForLinux__x64__8wekyb3d8bbwe",
            "MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3.0_x64_8wekyb3d8bbwe",
            "MicrosoftCorporationII.WindowsSubsystemForLinux_2.6.3.0_x64__8wekyb3d8bbwe_",
            "",
        ] {
            assert_eq!(package_folder_version(name), None, "{name}");
        }
    }

    #[test]
    fn the_msi_is_behind_only_when_its_version_is_older() {
        assert!(msi_behind((2, 6, 3), Some("2.6.1.0")));
        assert!(msi_behind((2, 6, 3), Some("2.5.9")));
        for msi in [
            Some("2.6.3"),
            Some("2.6.3.0"),
            Some("3.0.0"),
            Some(""),
            Some("x"),
            None,
        ] {
            assert!(!msi_behind((2, 6, 3), msi), "{msi:?}");
        }
    }
}
