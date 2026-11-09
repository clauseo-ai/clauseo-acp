//! The words of each outcome's log line.

use super::{Outcome, Refused, SOURCE_DIR};

/// A version as `a.b.c.d`.
fn dotted(version: [u16; 4]) -> String {
    let [a, b, c, d] = version;
    format!("{a}.{b}.{c}.{d}")
}

/// `outcome`'s log line.
pub(super) fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::NotNeeded => "Codex's rg.exe imports no VC++ runtime DLL".to_owned(),
        Outcome::RgUnreadable(err) => {
            format!("Codex's rg.exe did not read, so no VC++ runtime was placed: {err}")
        }
        Outcome::AlreadyBeside(dll) => format!("{dll} is beside Codex's rg.exe already"),
        Outcome::Placed(dll, version) => format!(
            "{dll} {} (Microsoft-signed, ARM64) copied beside Codex's rg.exe",
            dotted(*version)
        ),
        Outcome::Refused(dll, refused) => format!(
            "{dll} not placed beside Codex's rg.exe, which may not start without it: {}",
            refused_text(refused)
        ),
        Outcome::Unsupported(dll) => {
            format!("Codex's rg.exe imports {dll}, which Clauseo does not place; it may not start")
        }
        Outcome::Failed(dll, detail) => format!("{dll} beside Codex's rg.exe: {detail}"),
    }
}

/// Why, in words.
fn refused_text(refused: &Refused) -> String {
    match refused {
        Refused::Missing => format!("the package has no copy in {SOURCE_DIR}"),
        Refused::NotPe(err) => format!("the package's copy is not a PE file ({err})"),
        Refused::WrongArch(machine) => {
            format!("the package's copy is not ARM64 (machine 0x{machine:04X})")
        }
        Refused::NotTrusted(why) => format!("the package's copy has no valid signature ({why})"),
        Refused::NotMicrosoft(who) => {
            format!("the package's copy is not signed by Microsoft ({who})")
        }
        Refused::NoVersion => "the package's copy has no version resource".to_owned(),
        Refused::WrongName(name) => {
            format!("the package's copy is another file renamed (its original name is {name:?})")
        }
        Refused::NotRuntime(version) => format!(
            "the package's copy is version {}, not a VC++ 14.x runtime",
            dotted(*version)
        ),
        Refused::TooOld(version, (major, minor)) => format!(
            "the package's copy is version {}, older than rg.exe's toolset {major}.{minor}",
            dotted(*version)
        ),
        Refused::UnknownToolset((major, minor)) => {
            format!("rg.exe's linker version {major}.{minor} is not a VC++ 14.x toolset")
        }
    }
}
