//! Windows' own system names for a jailed agent (R17 PR-D2a round 3b): `ComSpec`, the drive,
//! the program folders, the processor's and `OS`, each from the OS (the known folders, the real
//! Windows folder, `GetNativeSystemInfo`), never from the lawyer's environment. A name whose value
//! cannot be read is left unset and counted, never inherited.
//!
//! [`system_vars`] is a plain function of what the OS said ([`Native`]), so its rules are tested
//! on every OS; only [`native`] reads Windows.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_settings::KnownFolder;

/// The program folders' names and the known folder each comes from.
pub(crate) const PROGRAM_FOLDERS: [(&str, KnownFolder); 7] = [
    ("ProgramData", KnownFolder::ProgramData),
    ("ProgramFiles", KnownFolder::ProgramFiles),
    ("ProgramFiles(x86)", KnownFolder::ProgramFilesX86),
    ("ProgramW6432", KnownFolder::ProgramFilesX64),
    ("CommonProgramFiles", KnownFolder::ProgramFilesCommon),
    (
        "CommonProgramFiles(x86)",
        KnownFolder::ProgramFilesCommonX86,
    ),
    ("CommonProgramW6432", KnownFolder::ProgramFilesCommonX64),
];

/// What the OS said, each `None` when it could not be read.
pub(crate) struct Native<'a> {
    /// The real Windows folder (`FOLDERID_Windows`).
    pub(crate) root: Option<PathBuf>,
    /// A known folder's path.
    pub(crate) folder: &'a dyn Fn(KnownFolder) -> Option<PathBuf>,
    /// `wProcessorArchitecture` from `GetNativeSystemInfo`.
    pub(crate) arch: Option<u16>,
    /// `dwNumberOfProcessors` from `GetNativeSystemInfo`.
    pub(crate) processors: Option<u32>,
}

/// The system names' values from `native`, in a fixed order, and how many names had none.
pub(crate) fn system_vars(native: &Native<'_>) -> (Vec<(OsString, OsString)>, usize) {
    let mut vars: Vec<(&str, Option<OsString>)> = vec![
        (
            "ComSpec",
            native
                .root
                .as_ref()
                .map(|root| root.join("System32").join("cmd.exe").into_os_string()),
        ),
        ("SystemDrive", native.root.as_deref().and_then(drive)),
    ];
    vars.extend(
        PROGRAM_FOLDERS
            .iter()
            .map(|(name, known)| (*name, (native.folder)(*known).map(PathBuf::into_os_string))),
    );
    vars.push((
        "PROCESSOR_ARCHITECTURE",
        native.arch.and_then(arch_name).map(OsString::from),
    ));
    vars.push((
        "NUMBER_OF_PROCESSORS",
        native
            .processors
            .filter(|count| *count > 0)
            .map(|count| OsString::from(count.to_string())),
    ));
    vars.push(("OS", Some(OsString::from("Windows_NT"))));
    let unset = vars.iter().filter(|(_, value)| value.is_none()).count();
    let set = vars
        .into_iter()
        .filter_map(|(name, value)| Some((OsString::from(name), value?)))
        .collect();
    (set, unset)
}

/// The names [`system_vars`] sets when the OS gives every value (the tests' list).
#[cfg(test)]
pub(crate) const SYSTEM_NAMES: [&str; 12] = [
    "ComSpec",
    "SystemDrive",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "CommonProgramW6432",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
    "OS",
];

/// `PROCESSOR_ARCHITECTURE`'s word for a `wProcessorArchitecture` value; `None` for one Windows
/// gives no word for here (`PROCESSOR_ARCHITECTURE_UNKNOWN` and the retired ones).
pub(crate) fn arch_name(arch: u16) -> Option<&'static str> {
    match arch {
        0 => Some("x86"),
        5 => Some("ARM"),
        9 => Some("AMD64"),
        12 => Some("ARM64"),
        _ => None,
    }
}

/// `C:` from `C:\Windows`, read as text (so the rule is tested on every OS); `None` for a
/// folder that does not start with a drive letter.
fn drive(root: &Path) -> Option<OsString> {
    match root.as_os_str().as_encoded_bytes() {
        [letter, b':', ..] if letter.is_ascii_alphabetic() => {
            Some(OsString::from(format!("{}:", char::from(*letter))))
        }
        _ => None,
    }
}

/// The system names from this machine: [`system_vars`] over the known folders, the real Windows
/// folder and `GetNativeSystemInfo`.
#[cfg(windows)]
pub(crate) fn native() -> (Vec<(OsString, OsString)>, usize) {
    use clauseo_settings::{PlatformDirs as _, SystemDirs};
    let (arch, processors) = native_info();
    system_vars(&Native {
        root: super::system_root(),
        folder: &|known| SystemDirs.known_folder(known),
        arch: Some(arch),
        processors: Some(processors),
    })
}

/// The native processor architecture and count (`GetNativeSystemInfo`: the machine's, not this
/// process's under emulation).
#[cfg(windows)]
#[expect(unsafe_code, reason = "FFI to GetNativeSystemInfo")]
fn native_info() -> (u16, u32) {
    use ::windows::Win32::System::SystemInformation::{GetNativeSystemInfo, SYSTEM_INFO};
    let mut info = SYSTEM_INFO::default();
    // SAFETY: `info` is a valid, writable SYSTEM_INFO for the whole call, which only fills it.
    unsafe { GetNativeSystemInfo(&raw mut info) };
    // SAFETY: every variant of the union is plain integers, and GetNativeSystemInfo fills the
    // struct form (`wProcessorArchitecture`); reading it is reading initialised bytes.
    let arch = unsafe { info.Anonymous.Anonymous.wProcessorArchitecture.0 };
    (arch, info.dwNumberOfProcessors)
}

#[cfg(test)]
mod tests;
