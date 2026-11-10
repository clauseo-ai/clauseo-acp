//! The rights an entry gives or denies, and their masks.

use serde::{Deserialize, Serialize};

use super::{LOW_LABEL_SID, OWNER_RIGHTS_SID};

/// The rights an entry gives or denies. Stored in the ledger by its snake-case name; new rights
/// are appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rights {
    /// Allow `Modify` (`0x1301bf`): read, write, run and delete, without `FILE_DELETE_CHILD`,
    /// `WRITE_DAC` or `WRITE_OWNER`, so the agent can't strip a deny or delete a child through
    /// its parent.
    Modify,
    /// Allow `ReadAndExecute` (`0x1200a9`).
    ReadAndExecute,
    /// Allow `CreateFiles` (`FILE_ADD_FILE`, `0x2`) on a folder: make a file in it, nothing more.
    CreateFiles,
    /// Deny writing (`0xd0156`): data, append, attributes, extended attributes, `DELETE`,
    /// `FILE_DELETE_CHILD`, `WRITE_DAC` and `WRITE_OWNER`.
    DenyWrite,
    /// Deny reading and writing (`0xf01ff`): [`Rights::DenyWrite`] plus read data, read
    /// attributes, read extended attributes, execute and `READ_CONTROL`.
    DenyReadWrite,
    /// Deny reading (`0x200a9`): read data, read attributes, read extended attributes, execute
    /// and `READ_CONTROL`.
    DenyRead,
    /// Not an access entry: the Low mandatory label `S:(ML;OICI;NW;;;LW)` (mask `0x1`,
    /// no-write-up), set with `LABEL_SECURITY_INFORMATION`; its subject is [`LOW_LABEL_SID`].
    LowLabel,
    /// Read and write the file's data, no `DELETE` and no permission change
    /// (`FILE_GENERIC_READ | FILE_WRITE_DATA | FILE_APPEND_DATA`, `0x12008f`): the credential
    /// link's allow, which lands on the lawyer's real credentials file (L5).
    ReadWriteData,
    /// Deny running (`FILE_EXECUTE`, `0x20`): on a file, executing it or mapping it as an image
    /// (an `.exe`, `.dll`, `.node`, `.pyd`). Written for files only (see `guard.rs`), since on a
    /// folder the same bit is traverse.
    DenyExecute,
    /// Not a subject's allow: `OWNER RIGHTS` ([`OWNER_RIGHTS_SID`]) given `Modify`'s mask, which
    /// replaces an owner's implied `READ_CONTROL | WRITE_DAC`, so a pool user that made a file
    /// can't change its permissions.
    OwnerModify,
    /// Deny changing permissions and owner (`WRITE_DAC | WRITE_OWNER`, `0xc0000`): against a
    /// `CREATOR OWNER` allow a matter inherits from `C:\`, which gives the creator both.
    DenyPermissionChange,
}

impl Rights {
    /// The access mask.
    pub fn mask(self) -> u32 {
        match self {
            Rights::Modify => 0x0013_01bf,
            Rights::ReadAndExecute => 0x0012_00a9,
            Rights::CreateFiles => 0x0000_0002,
            Rights::DenyWrite => DENY_WRITE,
            Rights::DenyReadWrite => DENY_WRITE | DENY_READ,
            Rights::DenyRead => DENY_READ,
            Rights::LowLabel => 0x1,
            Rights::ReadWriteData => 0x0012_008f,
            Rights::DenyExecute => 0x20,
            Rights::OwnerModify => Rights::Modify.mask(),
            Rights::DenyPermissionChange => 0x000c_0000,
        }
    }

    /// Whether the entry is a deny entry.
    pub fn is_deny(self) -> bool {
        matches!(
            self,
            Rights::DenyWrite
                | Rights::DenyReadWrite
                | Rights::DenyRead
                | Rights::DenyExecute
                | Rights::DenyPermissionChange
        )
    }

    /// Whether the item is the mandatory label rather than an access entry.
    pub fn is_label(self) -> bool {
        self == Rights::LowLabel
    }

    /// Whether the entry allows a launch's subject access: not a deny, not the label, and not
    /// `OWNER RIGHTS`' entry, which names whoever owns each item.
    pub fn is_allow(self) -> bool {
        !self.is_deny() && self.fixed_subject().is_none()
    }

    /// The one SID an item of these rights may name, when it is fixed: the Low level for the
    /// label, `OWNER RIGHTS` for [`Rights::OwnerModify`]. Every other entry names a subject.
    pub fn fixed_subject(self) -> Option<&'static str> {
        match self {
            Rights::LowLabel => Some(LOW_LABEL_SID),
            Rights::OwnerModify => Some(OWNER_RIGHTS_SID),
            Rights::Modify
            | Rights::ReadAndExecute
            | Rights::CreateFiles
            | Rights::DenyWrite
            | Rights::DenyReadWrite
            | Rights::DenyRead
            | Rights::ReadWriteData
            | Rights::DenyExecute
            | Rights::DenyPermissionChange => None,
        }
    }
}

/// `FILE_WRITE_DATA | FILE_APPEND_DATA | FILE_WRITE_EA | FILE_DELETE_CHILD |
/// FILE_WRITE_ATTRIBUTES | DELETE | WRITE_DAC | WRITE_OWNER`.
const DENY_WRITE: u32 = 0x2 | 0x4 | 0x10 | 0x40 | 0x100 | 0x0001_0000 | 0x0004_0000 | 0x0008_0000;

/// `FILE_READ_DATA | FILE_READ_EA | FILE_EXECUTE | FILE_READ_ATTRIBUTES | READ_CONTROL`.
const DENY_READ: u32 = 0x1 | 0x8 | 0x20 | 0x80 | 0x0002_0000;
