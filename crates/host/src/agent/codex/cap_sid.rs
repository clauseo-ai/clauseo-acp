//! Windows: Codex's sandbox SIDs file in its home, `cap_sid` (codex `rust-v0.162.1`,
//! `windows-sandbox-rs/src/cap.rs:35-80, 92-117`). Codex reads the SIDs it holds, makes each a
//! capability on the restricted token its sandboxed commands run under, and grants each write on
//! the session's writable folders (`token.rs:468-505`, a `WRITE_RESTRICTED` token: a write passes
//! where the user and one of those SIDs may both write). Codex makes each SID itself, at random,
//! as `S-1-5-21-` and four numbers. A SID the user holds put there instead (its own, a group's, or
//! any account or group of its machine or domain) would let a sandboxed command write wherever
//! the user may. So each launch sets the entry aside ([`check`]) unless it is a plain file with
//! one name, readable, whose every SID is one Codex could have made and none the user holds;
//! Codex then makes new ones, as on its first run. Scope: an approved command that runs outside
//! the sandbox on Windows can change what the sandbox reaches; the launch check does not undo
//! that.

#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
use crate::error::Error;
#[cfg(windows)]
use crate::log_file::Cause;

#[cfg(windows)]
use super::home_failed;

/// The file's name in Codex's home.
#[cfg(windows)]
const FILE: &str = "cap_sid";

/// The longest file read, in bytes; a longer one is not Codex's.
#[cfg(windows)]
const MAX_FILE: u64 = 1024 * 1024;

/// The start every SID Codex makes has (`cap.rs:39-46`).
const CODEX_SID_START: &str = "S-1-5-21-";

/// Sets `<home>/cap_sid` aside, as the launch's clearing does (`home_clear::set_aside`), then
/// removes it best-effort, unless it is Codex's own: an entry that [`read`] gives as text and
/// [`foreign`] passes against this process's SIDs (`own::sids`). One warning, no SID, no path.
/// Only an entry that can't be renamed refuses the launch ([`home_failed`]); none there is fine.
/// Windows only: Codex's sandbox elsewhere reads no such file.
#[cfg(windows)]
pub(crate) fn check(home: &Path) -> Result<(), Error> {
    let path = home.join(FILE);
    let text = match read(&path) {
        Found::Absent => return Ok(()),
        Found::Text(text) => Some(text),
        Found::Unusable => None,
    };
    if let Some(text) = text {
        match own::sids() {
            Ok(held) if !foreign(&text, &held) => return Ok(()),
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(error_kind = ?error.kind(), "this process's SIDs could not be read");
            }
        }
    }
    let Some(aside) = super::home_clear::set_aside(home, FILE).map_err(|error| failed(&error))?
    else {
        return Ok(());
    };
    let removed = super::home_clear::remove_entry(&aside).is_ok();
    if removed {
        tracing::warn!("Codex's sandbox SIDs file was not Codex's own and was set aside");
    } else {
        tracing::warn!(
            "Codex's sandbox SIDs file was not Codex's own and was set aside but not emptied: it \
             stays under a .clauseo-aside- name, where Codex loads nothing"
        );
    }
    Ok(())
}

/// What [`read`] found at `cap_sid`.
#[cfg(windows)]
enum Found {
    /// Nothing there.
    Absent,
    /// A plain file with one name, read whole: its text.
    Text(String),
    /// Anything else: a link or another reparse point, a folder, a file with more than one name,
    /// one that can't be opened or read, longer than [`MAX_FILE`], or not UTF-8.
    Unusable,
}

/// The entry at `path`, looked at itself first, never followed (`symlink_metadata`), then
/// opened as itself (`write::open_read_nofollow`, `FILE_FLAG_OPEN_REPARSE_POINT`) and checked
/// again on that handle (its attributes, and its count of names, `NumberOfLinks`:
/// `network_paths::link_count`) before it is read from it.
#[cfg(windows)]
fn read(path: &Path) -> Found {
    use std::io::Read as _;
    use std::os::windows::fs::MetadataExt as _;
    let meta = match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Found::Absent,
        Err(_) => return Found::Unusable,
        Ok(meta) => meta,
    };
    if !meta.file_type().is_file() || !plain(meta.file_attributes()) {
        return Found::Unusable;
    }
    let Ok(file) = crate::write::open_read_nofollow(path) else {
        return Found::Unusable;
    };
    let attributes = file.metadata().map(|meta| meta.file_attributes());
    let links = crate::network_paths::link_count(&file);
    if !matches!((attributes, links), (Ok(attributes), Ok(1)) if plain(attributes)) {
        return Found::Unusable;
    }
    let mut bytes = Vec::new();
    let cap = MAX_FILE.saturating_add(1);
    if file.take(cap).read_to_end(&mut bytes).is_err()
        || !u64::try_from(bytes.len()).is_ok_and(|len| len <= MAX_FILE)
    {
        return Found::Unusable;
    }
    String::from_utf8(bytes).map_or(Found::Unusable, Found::Text)
}

/// Whether file `attributes` are a plain file's: no folder, no reparse point.
#[cfg(windows)]
fn plain(attributes: u32) -> bool {
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    };
    attributes & (FILE_ATTRIBUTE_DIRECTORY.0 | FILE_ATTRIBUTE_REPARSE_POINT.0) == 0
}

/// Whether the text of a `cap_sid` file holds a SID Codex could not have made, or one in `held`
/// (the user's own SIDs, `own::sids`) or of the same machine or domain as one of them. Read as
/// Codex reads it (`cap.rs:62-80`): a JSON object with `workspace`, `readonly` and the two maps
/// of SIDs; any other non-empty text is one SID. Empty text, or an object Codex would not parse,
/// makes Codex write new SIDs over it: not foreign.
pub(crate) fn foreign(text: &str, held: &[String]) -> bool {
    let text = text.trim();
    let sids: Vec<String> = if text.starts_with('{') && text.ends_with('}') {
        match serde_json::from_str::<Caps>(text) {
            Ok(caps) => [caps.workspace, caps.readonly]
                .into_iter()
                .chain(caps.workspace_by_cwd.into_values())
                .chain(caps.writable_root_by_path.into_values())
                .collect(),
            Err(_) => return false,
        }
    } else if text.is_empty() {
        return false;
    } else {
        vec![text.to_owned()]
    };
    let domains: Vec<[u32; 3]> = held.iter().map(String::as_str).filter_map(domain).collect();
    sids.iter().any(|sid| {
        held.iter().any(|own| own.eq_ignore_ascii_case(sid))
            || codex_parts(sid).is_none_or(|[a, b, c, _]| domains.contains(&[a, b, c]))
    })
}

/// The file's shape, as Codex's `CapSids` (`cap.rs:15-32`).
#[derive(serde::Deserialize)]
struct Caps {
    workspace: String,
    readonly: String,
    #[serde(default)]
    workspace_by_cwd: std::collections::HashMap<String, String>,
    #[serde(default)]
    writable_root_by_path: std::collections::HashMap<String, String>,
}

/// The four numbers of a SID shaped as Codex makes one, `S-1-5-21-a-b-c-d`, each written as a
/// `u32` is (no sign, no leading zero); none for any other text.
fn codex_parts(sid: &str) -> Option<[u32; 4]> {
    let rest = sid.strip_prefix(CODEX_SID_START)?;
    let mut parts = [0_u32; 4];
    let mut count = 0_usize;
    for part in rest.split('-') {
        let number: u32 = part.parse().ok()?;
        if number.to_string() != part {
            return None;
        }
        *parts.get_mut(count)? = number;
        count = count.saturating_add(1);
    }
    (count == 4).then_some(parts)
}

/// The machine or domain part of a SID the user holds, `S-1-5-21-a-b-c-…`: its first three
/// numbers; none for a SID of another shape (a well-known one).
fn domain(sid: &str) -> Option<[u32; 3]> {
    let rest = sid.strip_prefix(CODEX_SID_START)?;
    let mut numbers = rest.split('-').map(|part| part.parse::<u32>().ok());
    Some([numbers.next()??, numbers.next()??, numbers.next()??])
}

/// The failure of a Codex launch whose `cap_sid` could not be set aside.
#[cfg(windows)]
fn failed(error: &std::io::Error) -> Error {
    tracing::warn!(error_kind = ?error.kind(), "Codex's sandbox SIDs file could not be set aside");
    home_failed(&Cause(error))
}

/// This process's user and group SIDs, as text.
#[cfg(windows)]
mod own {
    #![expect(unsafe_code, reason = "FFI to the Win32 token and SID calls")]

    use std::io;

    use windows::Win32::Foundation::{
        CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, HLOCAL, LocalFree,
    };
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::{
        GetTokenInformation, PSID, SID_AND_ATTRIBUTES, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS,
        TOKEN_QUERY, TOKEN_USER, TokenGroups, TokenUser,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::{HRESULT, PWSTR};

    /// The token's user SID, then each of its groups' SIDs.
    pub(super) fn sids() -> io::Result<Vec<String>> {
        let token = Token::open()?;
        let mut out = Vec::new();
        let user = token.variable::<TOKEN_USER>(TokenUser)?;
        // SAFETY: GetTokenInformation wrote a TOKEN_USER at the start of `user`, at least that
        // long and aligned for it (u64 words); its SID points into `user`, alive to the end.
        let user_sid = unsafe { &*user.as_ptr().cast::<TOKEN_USER>() }.User.Sid;
        out.push(text(user_sid)?);
        let groups = token.variable::<TOKEN_GROUPS>(TokenGroups)?;
        for entry in entries(&groups)? {
            out.push(text(entry.Sid)?);
        }
        Ok(out)
    }

    /// The group entries of the `TOKEN_GROUPS` that `GetTokenInformation` wrote at the start of
    /// `groups`, taken from the buffer itself (never past the one-entry array the type names),
    /// once its count of entries is checked to fit in it.
    fn entries(groups: &[u64]) -> io::Result<&[SID_AND_ATTRIBUTES]> {
        // SAFETY: a TOKEN_GROUPS is at the start of `groups`, at least that long and aligned
        // for it (u64 words); only its count is read through this pointer.
        let count = unsafe { (*groups.as_ptr().cast::<TOKEN_GROUPS>()).GroupCount };
        let count = usize::try_from(count).map_err(io::Error::other)?;
        let offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
        let fits = count
            .checked_mul(size_of::<SID_AND_ATTRIBUTES>())
            .and_then(|entries| entries.checked_add(offset))
            .is_some_and(|end| end <= size_of_val(groups));
        if !fits {
            return Err(io::Error::other("the token's groups overrun their buffer"));
        }
        // SAFETY: `offset` and the `count` entries after it are inside `groups` (checked above);
        // `offset` keeps the alignment the type's own field has; the token wrote `count` entries
        // there, each SID a pointer into `groups`, which the slice borrows.
        let entries = unsafe {
            let first = groups
                .as_ptr()
                .cast::<SID_AND_ATTRIBUTES>()
                .byte_add(offset);
            std::slice::from_raw_parts(first, count)
        };
        Ok(entries)
    }

    /// `sid` as text (`S-1-…`).
    fn text(sid: PSID) -> io::Result<String> {
        let mut wide = PWSTR::null();
        // SAFETY: `sid` is a valid SID inside a live buffer; `wide` is a live out-pointer.
        unsafe { ConvertSidToStringSidW(sid, &raw mut wide) }.map_err(io::Error::other)?;
        // SAFETY: on success `wide` is a NUL-terminated string that LocalAlloc made.
        let text = unsafe { wide.to_string() }.map_err(io::Error::other);
        // SAFETY: freed once, here; a failed free leaves nothing to do.
        unsafe { LocalFree(Some(HLOCAL(wide.0.cast()))) };
        text
    }

    /// This process's token, opened to query; closed on drop.
    struct Token(HANDLE);

    impl Token {
        fn open() -> io::Result<Self> {
            let mut handle = HANDLE::default();
            // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `handle` is
            // a live out-pointer for the call.
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut handle) }
                .map_err(io::Error::other)?;
            Ok(Self(handle))
        }

        /// The token's `class` information, in u64 words, at least `size_of::<T>()` bytes.
        fn variable<T>(&self, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<u64>> {
            let mut needed = 0_u32;
            // SAFETY: no buffer and a zero length ask only for the size, written to `needed`.
            let sized = unsafe { GetTokenInformation(self.0, class, None, 0, &raw mut needed) };
            match sized {
                Err(error) if error.code() == HRESULT::from_win32(ERROR_INSUFFICIENT_BUFFER.0) => {}
                Err(error) => return Err(io::Error::other(error)),
                Ok(()) => return Err(io::Error::other("the token gave no size")),
            }
            let bytes = usize::try_from(needed)
                .map_err(io::Error::other)?
                .max(size_of::<T>());
            let mut buffer = vec![0_u64; bytes.div_ceil(size_of::<u64>())];
            let len = u32::try_from(size_of_val(buffer.as_slice())).map_err(io::Error::other)?;
            let mut written = 0_u32;
            // SAFETY: `buffer` is live, writable and `len` bytes long for the call.
            unsafe {
                GetTokenInformation(
                    self.0,
                    class,
                    Some(buffer.as_mut_ptr().cast()),
                    len,
                    &raw mut written,
                )
            }
            .map_err(io::Error::other)?;
            Ok(buffer)
        }
    }

    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: the handle came from OpenProcessToken and is closed once, here.
            drop(unsafe { CloseHandle(self.0) });
        }
    }
}

#[cfg(test)]
mod tests;
// Two attributes, not `all(test, windows)`: clippy counts a module as test code (unwraps and
// prints allowed, clippy.toml) only under a plain `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod windows_tests;
