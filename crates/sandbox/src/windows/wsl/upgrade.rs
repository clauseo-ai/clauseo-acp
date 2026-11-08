//! A pending WSL upgrade, as WSL's own installer judges it (`WslInstaller.cpp:114-133`,
//! `IsUpdateNeeded`, at the commit 0049 cites): the Store package's `WslInstaller` service
//! upgrades through the MSI when `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Lxss\MSI`'s
//! `Version` is empty or older than the package's own version (major, minor, revision). Any
//! user may start that upgrade: the package's packaged COM class `WslInstaller` grants launch to
//! Authenticated Users (`AppxManifest.in:74-78`), and the MSI rewrites WSL's launch permissions,
//! dropping the deny until the refresh puts it back. So a tier B launch is refused while one is
//! pending ([`Blocked::WslUpgradePending`]).
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
//! upgrade refuses the launch whoever could start it.
//!
//! [`Blocked::WslUpgradePending`]: crate::Blocked::WslUpgradePending

use super::seam::WslRead;

/// The package's name, the start of each of its full names.
pub const PACKAGE_NAME: &str = "MicrosoftCorporationII.WindowsSubsystemForLinux";

/// A version as WSL compares them: major, minor, revision.
type Version = (u32, u32, u32);

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
