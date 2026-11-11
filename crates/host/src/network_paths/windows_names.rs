//! Windows' names that reach somewhere other than their text says, refused from an agent's text
//! alone before anything touches it, and the pure halves of the Windows checks (a drive's device,
//! a reparse point's kind), so each is tested on every system.
//!
//! Refused names (with Windows' rules): a trailing dot or space (the system trims it, so
//! `x.` opens `x`), a `:` anywhere but a drive letter's (an alternate data stream; a drive
//! written inside a word, `grep -r x D:` of a script handed whole to a shell, is no stream and
//! not refused: [`drive_in_a_word`]), and a
//! reserved device name
//! (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`, with any extension, in any
//! case). A wildcard (`*`, `?`) is not refused: such a word is walked by its text, and the
//! system answers that the name is invalid. A URL is not refused either: the file system reads
//! it as a relative path, and `https:` is answered the same way.

use std::path::Path;

use super::{NetEnv, is_url};

/// The reserved device names, without their digit.
const DEVICES: [&str; 4] = ["con", "prn", "aux", "nul"];

/// Whether the agent's text `path` holds a name Windows reads as another place (above). Only
/// with Windows' rules; never for a URL.
pub(crate) fn refused_windows_name(path: &Path, net: &NetEnv) -> bool {
    if !net.windows {
        return false;
    }
    let text = net.plain(path).to_string_lossy().into_owned();
    if is_url(&text) {
        return false;
    }
    let mut names = text.split(['/', '\\']).filter(|name| !name.is_empty());
    let first_is_drive = text.as_bytes().get(1) == Some(&b':')
        && text.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
    if first_is_drive {
        let drive = names.next().unwrap_or_default();
        // `C:` alone, or `C:x` (drive-relative): the rest after the colon is a name too.
        if let Some(rest) = drive.get(2..).filter(|rest| !rest.is_empty())
            && refused(rest)
        {
            return true;
        }
    }
    names.any(refused)
}

/// Whether `name`'s only `:` is a drive written inside a word, the name's last character after
/// a letter that follows a blank, a quote, a bracket, `=`, `,` or `;`: the `grep -r x D:` of a
/// script handed whole to `bash -c`, or the `exec(open('D:` of a script's string, cut at the
/// drive's separator. No alternate data stream: a stream has a name or a type after its `:`
/// (`x:s`, `x::$DATA`, `x:$I30:$INDEX_ALLOCATION`), and those stay refused, as does a bare
/// `x:` name. The system answers that such a name is invalid, so the walk keeps it as written
/// (no such name), and the drive path inside the word is read on its own (the script's words one
/// level down, the data folder's text anywhere in a word).
fn drive_in_a_word(name: &str) -> bool {
    let Some(head) = name.strip_suffix(':') else {
        return false;
    };
    let mut before = head.chars().rev();
    !head.contains(':')
        && before.next().is_some_and(|c| c.is_ascii_alphabetic())
        && before
            .next()
            .is_some_and(|c| c.is_whitespace() || "'\"([{=,;`".contains(c))
}

/// Whether one name is refused (the module's rules).
fn refused(name: &str) -> bool {
    if name == "." || name == ".." {
        return false;
    }
    if name.ends_with('.') || name.ends_with(' ') || (name.contains(':') && !drive_in_a_word(name))
    {
        return true;
    }
    let base = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_lowercase();
    DEVICES.contains(&base.as_str())
        || (base.len() == 4
            && (base.starts_with("com") || base.starts_with("lpt"))
            && base
                .as_bytes()
                .get(3)
                .is_some_and(|d| (b'1'..=b'9').contains(d)))
}

/// Whether `QueryDosDeviceW`'s answer for a drive letter is a local volume: a hard disk volume
/// or a CD drive. Anything else (`\Device\Mup`, `\??\UNC`, `\Device\LanmanRedirector`, any
/// other redirector) counts as remote; a `subst` to a path is followed by [`drive_is_local`].
#[cfg(any(windows, test))]
pub(crate) fn drive_target_is_local(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    ["\\device\\harddiskvolume", "\\device\\cdrom"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

/// The most `subst` drives one drive letter may lead through; past it, or in a loop, the drive
/// counts as remote.
#[cfg(any(windows, test))]
const MAX_SUBST_HOPS: usize = 8;

/// Whether drive `letter` is local, its device asked by `query` (`QueryDosDeviceW`'s first
/// target; `None` when it can't be asked): a local volume ([`drive_target_is_local`]), or a
/// `subst` (`\??\X:\folder`) to a drive that is, followed up to [`MAX_SUBST_HOPS`] times. A
/// loop, too many hops, a `subst` to a UNC path (`\??\UNC\…`) and any other answer are remote.
#[cfg(any(windows, test))]
pub(crate) fn drive_is_local(letter: char, query: &dyn Fn(char) -> Option<String>) -> bool {
    let mut seen: Vec<char> = Vec::new();
    let mut letter = letter.to_ascii_uppercase();
    while seen.len() <= MAX_SUBST_HOPS && !seen.contains(&letter) {
        seen.push(letter);
        let Some(target) = query(letter) else {
            return false;
        };
        if drive_target_is_local(&target) {
            return true;
        }
        let Some(next) = subst_drive(&target) else {
            return false;
        };
        letter = next;
    }
    false
}

/// The drive a `subst` target `\??\X:\…` (or `\??\X:`) names, upper case.
#[cfg(any(windows, test))]
fn subst_drive(target: &str) -> Option<char> {
    let rest = target.strip_prefix(r"\??\")?;
    let mut chars = rest.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    let after = chars.as_str().strip_prefix(':')?;
    (after.is_empty() || after.starts_with('\\')).then(|| letter.to_ascii_uppercase())
}

/// Whether a reparse point's tag that is not a link (a symlink or a junction, which the walk
/// reads as links) is a known local kind: data deduplication, the cloud files placeholders
/// (`OneDrive`), WOF compression, a container's layer (WCI), an app execution alias, `ProjFS` (VFS
/// for Git) and an `AF_UNIX` socket. Any other (DFS, Azure File Sync, an unknown filter) is
/// refused.
#[cfg(any(windows, test))]
pub(crate) fn local_reparse_tag(tag: u32) -> bool {
    /// `IO_REPARSE_TAG_CLOUD` and its sixteen variants (`0x9000_x01A`).
    const CLOUD: u32 = 0x9000_001A;
    const CLOUD_MASK: u32 = 0xFFFF_0FFF;
    const LOCAL: [u32; 6] = [
        0x8000_0013, // IO_REPARSE_TAG_DEDUP
        0x8000_0017, // IO_REPARSE_TAG_WOF
        0x8000_0018, // IO_REPARSE_TAG_WCI
        0x8000_001B, // IO_REPARSE_TAG_APPEXECLINK
        0x9000_001C, // IO_REPARSE_TAG_PROJFS
        0x8000_0023, // IO_REPARSE_TAG_AF_UNIX
    ];
    tag & CLOUD_MASK == CLOUD || LOCAL.contains(&tag)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{drive_is_local, drive_target_is_local, local_reparse_tag, refused_windows_name};
    use crate::network_paths::tests::plain_net;

    #[test]
    fn names_windows_reads_as_another_place_are_refused() {
        let net = plain_net(true);
        for text in [
            r"C:\m\brief.docx.",
            r"C:\m\brief.docx ",
            r"C:\m\brief.docx:secret",
            r"C:\m\x:\y",
            r"C:\m\D:\x",
            r"C:\m\x D:s\y",
            r"C:\m\x D::$DATA",
            r"C:\m\data::$INDEX_ALLOCATION\x",
            r"C:\m\data:$I30:$INDEX_ALLOCATION\x",
            r"C:\m\x D:.",
            r"C:\m\xD:\y",
            r"C:\m\con",
            r"C:\m\CON.txt",
            r"C:\m\nul.tar.gz",
            r"C:\m\Com1",
            r"C:\m\lpt9.x",
            r"C:\m\aux .txt",
            "docs/x./y",
        ] {
            assert!(refused_windows_name(Path::new(text), &net), "{text}");
        }
        for text in [
            r"C:\m\brief.docx",
            r"C:\m\..\x",
            r"docs\*.docx",
            r"docs\?.docx",
            "https://example.com/a",
            r"\\?\C:\m\x",
            r"C:\m\console.txt",
            r"C:\m\com10",
            r"C:\m\a~b",
            r"C:\m\~1x",
            r"C:\PROGRA~1\x",
            r"C:\Users\JONATH~1\AppData\Local\Temp\x",
            r"C:\m\Draft~2.docx",
            // A drive inside a word, cut at its separator (#766's Windows CI): a script handed
            // whole to a shell, a drive in a script's string.
            "grep -r x D:/a/_temp/t/matter",
            "ls D:/a/t/outside && wc -l D:/a/t/matter/sub/a.txt",
            r"exec(open('D:\a\t\data/run/s1/x/run.py').read())",
            r#"print(open("C:/m/x").read())"#,
            "--file=D:/m/x",
        ] {
            assert!(!refused_windows_name(Path::new(text), &net), "{text}");
        }
        assert!(!refused_windows_name(
            Path::new(r"C:\m\x."),
            &plain_net(false)
        ));
    }

    #[test]
    fn a_drive_is_local_only_on_a_local_volume_and_a_reparse_point_only_of_a_local_kind() {
        assert!(drive_target_is_local(r"\Device\HarddiskVolume3"));
        assert!(drive_target_is_local(r"\Device\CdRom0"));
        for target in [
            r"\Device\Mup\;Z:000\server\share",
            r"\??\UNC\server\share",
            r"\Device\LanmanRedirector\;Z:000\server\share",
            r"\Device\WebDavRedirector\x",
            r"\??\C:\folder",
            "",
        ] {
            assert!(!drive_target_is_local(target), "{target}");
        }
        for tag in [0x8000_0013, 0x9000_001A, 0x9000_F01A, 0x8000_0017] {
            assert!(local_reparse_tag(tag), "{tag:#x}");
        }
        for tag in [0x8000_000A, 0x8000_001E, 0x1234] {
            assert!(!local_reparse_tag(tag), "{tag:#x}");
        }
    }

    /// A `subst` to a local folder is followed to its drive (`\??\C:\folder`); one to a UNC
    /// path, a loop, too many hops or a drive that can't be asked is remote.
    #[test]
    fn a_subst_drive_is_followed_to_the_drive_it_names() {
        let query = |letter: char| -> Option<String> {
            Some(
                match letter {
                    'C' => r"\Device\HarddiskVolume3",
                    'S' => r"\??\C:\Users\a\Deals",
                    'T' => r"\??\S:\sub",
                    'U' => r"\??\UNC\server\share",
                    'V' => r"\??\U:\x",
                    'L' => r"\??\M:\x",
                    'M' => r"\??\L:\x",
                    'Z' => r"\Device\Mup\;Z:000\server\share",
                    // A chain past the hop limit: D -> E -> ... -> N's neighbour.
                    'D'..='K' => {
                        let next = char::from(u8::try_from(letter).unwrap() + 1);
                        return Some(format!(r"\??\{next}:\x"));
                    }
                    _ => return None,
                }
                .to_owned(),
            )
        };
        for letter in ['C', 'c', 'S', 'T'] {
            assert!(drive_is_local(letter, &query), "{letter}");
        }
        for letter in ['U', 'V', 'L', 'M', 'Z', 'N', 'D'] {
            assert!(!drive_is_local(letter, &query), "{letter}");
        }
    }
}
