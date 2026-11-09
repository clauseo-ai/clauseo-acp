//! The namespace's object names and security descriptors, as text, exactly as the W2 (j) recipe
//! made them on the VM (`/tmp/stagec/w2-j-recipe.md`, step 1). Plain Rust, so the tests check
//! them against the recipe on every OS.

use crate::handles::Slot;
use crate::package::Sid;

/// The full object name of `slot` in session `session` for the package SID `package`.
pub(crate) fn object_name(slot: Slot, session: u32, package: &Sid) -> String {
    let directory = directory(session, package);
    match slot.below_directory() {
        Some(below) => format!("{directory}{below}"),
        None => format!(r"\Device\NamedPipe{directory}"),
    }
}

/// `\Sessions\<n>\AppContainerNamedObjects\<package SID>`.
pub(crate) fn directory(session: u32, package: &Sid) -> String {
    format!(
        r"\Sessions\{session}\AppContainerNamedObjects\{}",
        package.as_str()
    )
}

/// The object a slot's directory shadows (`None` for the links and the pipe folder).
pub(crate) fn shadow(slot: Slot, session: u32) -> Option<String> {
    match slot {
        Slot::Directory => Some(format!(r"\Sessions\{session}\BaseNamedObjects")),
        Slot::RpcControl => Some(r"\RPC Control".to_owned()),
        Slot::Global => Some(r"\BaseNamedObjects".to_owned()),
        Slot::Local | Slot::Session | Slot::Pipes => None,
    }
}

/// The descriptor of the directories and links: the recipe's D1 (all access for SYSTEM, the
/// owner, the browser account and the package; inherited all access for SYSTEM, the creator
/// and the package; label Low, no write up), with the owner's user as the objects' owner (`O:`)
/// so the guard can check who made them, an elevated owner's included (whose default owner is
/// Administrators).
pub(crate) fn directory_sddl(owner: &Sid, browser: &Sid, package: &Sid) -> String {
    let (o, b, p) = (owner.as_str(), browser.as_str(), package.as_str());
    format!(
        "O:{o}D:(A;;GA;;;SY)(A;;GA;;;{o})(A;;GA;;;{b})(A;;GA;;;{p})\
         (A;OICIIO;GA;;;SY)(A;OICIIO;GA;;;CO)(A;OICIIO;GA;;;{p})S:(ML;;NW;;;LW)"
    )
}

/// What the browser account may do to the pipe folder: all file access (`FILE_ALL_ACCESS`,
/// `0x1F01FF`) but `DELETE`, `WRITE_DAC` and `WRITE_OWNER`, so no `B_s` process can delete the
/// folder or change its descriptor after the guard's check. The guard opens it with
/// `READ_CONTROL | SYNCHRONIZE` only.
pub(crate) const PIPES_BROWSER_MASK: u32 = 0x0012_01FF;

/// What the package may do in the pipe folder: `FILE_ADD_FILE` (`0x2`) and
/// `FILE_ADD_SUBDIRECTORY`, which for NPFS is `FILE_CREATE_PIPE_INSTANCE` (`0x4`), to make
/// pipes in it, with `READ_CONTROL | SYNCHRONIZE`; never list or read it, and no `DELETE`,
/// `WRITE_DAC` or `WRITE_OWNER` (the recipe's `0x1F0006` had them).
pub(crate) const PIPES_PACKAGE_MASK: u32 = 0x0012_0006;

/// The descriptor of the pipe folder: the recipe's F1 with `O:` as [`directory_sddl`], and the
/// browser account's and the package's access trimmed to [`PIPES_BROWSER_MASK`] and
/// [`PIPES_PACKAGE_MASK`]. The owner makes it with this, and the guard's check compares the
/// folder's DACL and label with this.
pub(crate) fn pipe_folder_sddl(owner: &Sid, browser: &Sid, package: &Sid) -> String {
    let (o, b, p) = (owner.as_str(), browser.as_str(), package.as_str());
    format!(
        "O:{o}D:(A;;FA;;;SY)(A;;FA;;;{o})(A;;{PIPES_BROWSER_MASK:#x};;;{b})\
         (A;;{PIPES_PACKAGE_MASK:#x};;;{p})\
         (A;OICIIO;GA;;;SY)(A;OICIIO;GA;;;CO)(A;OICIIO;GA;;;{p})S:(ML;;NW;;;LW)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{PackageName, package_sid};

    const OWNER: &str = "S-1-5-21-4086760546-1163544284-2580614524-1001";
    const BROWSER: &str = "S-1-5-21-4086760546-1163544284-2580614524-1002";

    fn b01() -> Sid {
        package_sid(&PackageName::new("Clauseo.Browser.B01").unwrap())
    }

    #[test]
    fn the_names_are_the_recipes() {
        let p = b01();
        let dir = format!(r"\Sessions\1\AppContainerNamedObjects\{}", p.as_str());
        let names: Vec<String> = Slot::ALL.iter().map(|s| object_name(*s, 1, &p)).collect();
        assert_eq!(
            names,
            [
                dir.clone(),
                format!(r"{dir}\RPC Control"),
                format!(r"{dir}\Global"),
                format!(r"{dir}\Local"),
                format!(r"{dir}\Session"),
                format!(r"\Device\NamedPipe{dir}"),
            ]
        );
        assert_eq!(directory(1, &p), dir);
    }

    /// Neither the browser account nor the package may delete the pipe folder or change its
    /// descriptor; the package may make pipes in it and no more.
    #[test]
    fn the_pipe_folder_masks_have_no_delete_dac_or_owner_rights() {
        // DELETE, WRITE_DAC, WRITE_OWNER.
        let forbidden = 0x0001_0000 | 0x0004_0000 | 0x0008_0000;
        assert_eq!(PIPES_BROWSER_MASK & forbidden, 0);
        assert_eq!(PIPES_PACKAGE_MASK & forbidden, 0);
        // FILE_ALL_ACCESS less the three.
        assert_eq!(PIPES_BROWSER_MASK, 0x001F_01FF & !forbidden);
        // FILE_ADD_FILE | FILE_CREATE_PIPE_INSTANCE, READ_CONTROL | SYNCHRONIZE.
        assert_eq!(PIPES_PACKAGE_MASK, 0x2 | 0x4 | 0x0002_0000 | 0x0010_0000);
    }

    #[test]
    fn the_shadows_are_the_recipes() {
        let shadows: Vec<Option<String>> = Slot::ALL.iter().map(|s| shadow(*s, 3)).collect();
        assert_eq!(
            shadows,
            [
                Some(r"\Sessions\3\BaseNamedObjects".to_owned()),
                Some(r"\RPC Control".to_owned()),
                Some(r"\BaseNamedObjects".to_owned()),
                None,
                None,
                None,
            ]
        );
    }

    /// D1 and F1, copied from the recipe's table, with the VM's SIDs; the crate adds `O:`, and
    /// trims F1's browser and package ACEs (`0x1201ff` for `FA`, `0x120006` for `0x1f0006`).
    const D1: &str = "D:(A;;GA;;;SY)(A;;GA;;;S-1-5-21-4086760546-1163544284-2580614524-1001)\
        (A;;GA;;;S-1-5-21-4086760546-1163544284-2580614524-1002)\
        (A;;GA;;;S-1-15-2-4291742471-3177126929-1079639061-2360262239-4224205492-3771709147-\
        1509459664)(A;OICIIO;GA;;;SY)(A;OICIIO;GA;;;CO)\
        (A;OICIIO;GA;;;S-1-15-2-4291742471-3177126929-1079639061-2360262239-4224205492-\
        3771709147-1509459664)S:(ML;;NW;;;LW)";
    const F1: &str = "D:(A;;FA;;;SY)(A;;FA;;;S-1-5-21-4086760546-1163544284-2580614524-1001)\
        (A;;0x1201ff;;;S-1-5-21-4086760546-1163544284-2580614524-1002)\
        (A;;0x120006;;;S-1-15-2-4291742471-3177126929-1079639061-2360262239-4224205492-\
        3771709147-1509459664)(A;OICIIO;GA;;;SY)(A;OICIIO;GA;;;CO)\
        (A;OICIIO;GA;;;S-1-15-2-4291742471-3177126929-1079639061-2360262239-4224205492-\
        3771709147-1509459664)S:(ML;;NW;;;LW)";

    #[test]
    fn the_descriptors_are_the_recipes() {
        let (o, b, p) = (
            Sid::parse(OWNER).unwrap(),
            Sid::parse(BROWSER).unwrap(),
            b01(),
        );
        assert_eq!(directory_sddl(&o, &b, &p), format!("O:{OWNER}{D1}"));
        assert_eq!(pipe_folder_sddl(&o, &b, &p), format!("O:{OWNER}{F1}"));
    }
}
