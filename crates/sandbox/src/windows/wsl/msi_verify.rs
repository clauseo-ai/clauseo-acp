//! The checks `wsl.msi` passes before the SYSTEM run hands it to msiexec (ADR 0049, "WSL",
//! "Self-healing"), so nothing a non-admin can place or change reaches a SYSTEM install:
//!
//! 1. **The folder**: the service's image is in a folder of WSL's package family, directly
//!    under `%ProgramFiles%\WindowsApps` ([`installer_package`], [`package_folder_version`]).
//! 2. **No reparse point**: `WindowsApps`, the package's folder and `wsl.msi` are each opened
//!    without following a reparse point and refused if one; `wsl.msi` is a regular file with one
//!    link ([`MsiFiles::open`]).
//! 3. **The owner**: `wsl.msi`'s, read on its handle, is `TrustedInstaller` or SYSTEM.
//! 4. **The signature**: `WinVerifyTrust` trusts it, and its signer's chain passes
//!    `CERT_CHAIN_POLICY_MICROSOFT_ROOT`: it ends in one of Microsoft's own roots; its leaf
//!    certificate lists the code-signing EKU; and its subject Organization is exactly
//!    `Microsoft Corporation` ([`leaf_org_ok`]).
//! 5. **The handle**: held, shared for reading only, until msiexec has ended, so the file can't
//!    be renamed, replaced or written between the checks and the install.
//!
//! Any check that fails is [`MsiUpgrade::Failed`], and the run re-applies the deny as after any
//! failed upgrade. The order of the checks and their verdicts hold no Windows call: they are
//! tested on every OS over [`MsiFiles`]; `msi_files.rs` is the real file system.
//!
//! [`package_folder_version`]: super::upgrade::package_folder_version

use super::seam::WslRead;
use super::upgrade::{PACKAGE_MSI, Version, installer_package, msi_behind};

/// `NT SERVICE\TrustedInstaller`, which owns what the Store installs.
pub(super) const TRUSTED_INSTALLER: &str =
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
/// `LOCAL SYSTEM`.
pub(super) const LOCAL_SYSTEM: &str = "S-1-5-18";

/// What [`upgrade_with`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MsiUpgrade {
    /// No upgrade was pending, or there is no `WslInstaller` service to take the MSI from.
    NotPending,
    /// The MSI at `from` was upgraded to the package's version `to`.
    Upgraded {
        /// The MSI's version before.
        from: String,
        /// The package's version.
        to: Version,
    },
    /// The upgrade was owed and failed, or couldn't be tried: why, with codes, never a path.
    Failed(String),
    /// msiexec was started and hadn't ended at its wait's end (`MSI_WAIT`): it was left running,
    /// as ending an installer mid-write could leave WSL half-changed. Why, never a path.
    StillInstalling(String),
}

/// Why an install didn't end in success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// msiexec didn't start, or ended with a failure: why, with codes.
    Failed(String),
    /// msiexec was still running at the end of its wait, and was left running.
    StillInstalling(String),
}

/// The file system's part of the checks. Every error is words and codes, never a path.
pub(super) trait MsiFiles {
    /// What stays open from the checks to the end of the install.
    type Held;
    /// `wsl.msi` in the folder `package` of `apps`: each of the three opened without following
    /// a reparse point and refused if one, the file refused unless a regular file with one link,
    /// and held open, shared for reading only.
    fn open(&self, apps: &str, package: &str) -> Result<Self::Held, String>;
    /// The held file's owner, as a string SID.
    fn owner(&self, msi: &Self::Held) -> Result<String, String>;
    /// The held file's signature, once `WinVerifyTrust` trusts it; `path` is the file's path,
    /// which the call takes alongside the handle.
    fn signature(&self, msi: &Self::Held, path: &str) -> Result<Signature, String>;
}

/// What a trusted signature's chain holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Signature {
    /// It passes `CERT_CHAIN_POLICY_MICROSOFT_ROOT`: it ends in one of Microsoft's own roots.
    pub(super) microsoft_root: bool,
    /// Its leaf certificate lists the code-signing EKU (`szOID_PKIX_KP_CODE_SIGNING`).
    pub(super) code_signing: bool,
    /// Its leaf certificate's subject Organization is `Microsoft Corporation` ([`leaf_org_ok`]).
    pub(super) leaf_org: bool,
}

/// The subject Organization a WSL MSI's leaf certificate must have.
const MICROSOFT_ORGANIZATION: &str = "Microsoft Corporation";

/// Whether a leaf certificate's subject Organization is `Microsoft Corporation`. `org` is the
/// value as the API returns it, or `None` when there is none or it can't be read; the match is
/// exact and case-sensitive, with nothing trimmed.
pub(super) fn leaf_org_ok(org: Option<&str>) -> bool {
    org == Some(MICROSOFT_ORGANIZATION)
}

/// Installs WSL's pending MSI upgrade, as `msi.rs` says, from the `WslInstaller` service's
/// `ImagePath` `image`, with `apps` `%ProgramFiles%\WindowsApps`. `install` gets the verified
/// MSI's path and its held handle, which is closed only after `install` returns.
pub(super) fn upgrade_with<F: MsiFiles>(
    files: &F,
    reg: &dyn WslRead,
    image: &str,
    apps: &str,
    install: impl FnOnce(&str, &F::Held) -> Result<(), InstallError>,
) -> MsiUpgrade {
    let Some((folder, package)) = installer_package(image, apps) else {
        let why = "the installer service's image isn't in a WSL package's folder";
        return MsiUpgrade::Failed(why.to_owned());
    };
    let msi = match reg.msi_version() {
        Ok(msi) => msi,
        Err(error) => return MsiUpgrade::Failed(format!("the MSI's version: {error}")),
    };
    if !msi_behind(package, msi.as_deref()) {
        return MsiUpgrade::NotPending;
    }
    // `installer_package` holds the folder to one name directly under `apps`.
    let name = folder.rsplit('\\').next().unwrap_or_default();
    let path = format!(r"{}\{name}\{PACKAGE_MSI}", apps.trim_end_matches('\\'));
    let held = match verified(files, apps, name, &path) {
        Ok(held) => held,
        Err(why) => return MsiUpgrade::Failed(why),
    };
    let installed = install(&path, &held);
    drop(held);
    match installed {
        Ok(()) => MsiUpgrade::Upgraded {
            from: msi.unwrap_or_default(),
            to: package,
        },
        Err(InstallError::Failed(why)) => MsiUpgrade::Failed(why),
        Err(InstallError::StillInstalling(why)) => MsiUpgrade::StillInstalling(why),
    }
}

/// The MSI, opened and held, once every check passes, as the module says.
fn verified<F: MsiFiles>(
    files: &F,
    apps: &str,
    package: &str,
    path: &str,
) -> Result<F::Held, String> {
    let held = files.open(apps, package)?;
    let owner = files.owner(&held)?;
    if owner != TRUSTED_INSTALLER && owner != LOCAL_SYSTEM {
        return Err(format!(
            "{PACKAGE_MSI}'s owner is neither TrustedInstaller nor SYSTEM"
        ));
    }
    let signature = files.signature(&held, path)?;
    if !signature.microsoft_root {
        return Err(format!(
            "{PACKAGE_MSI}'s signature doesn't chain to a Microsoft root"
        ));
    }
    if !signature.code_signing {
        return Err(format!(
            "{PACKAGE_MSI}'s signer isn't a code-signing certificate"
        ));
    }
    if !signature.leaf_org {
        return Err(format!(
            "{PACKAGE_MSI}'s signer's subject isn't Microsoft Corporation"
        ));
    }
    Ok(held)
}

#[cfg(test)]
mod tests;
