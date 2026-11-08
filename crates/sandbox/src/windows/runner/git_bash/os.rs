//! [`GitBashOs`] on Windows, as the desktop user: the record, the tree's checks through
//! handles (`git_tree::checked_chain`), the runtime's final path, and msys's folders made with
//! `NtCreateDirectoryObject`.
#![expect(
    unsafe_code,
    reason = "FFI to GetFinalPathNameByHandleW, GetLongPathNameW, ProcessIdToSessionId and \
              NtCreateDirectoryObject"
)]

use std::fs::File;
use std::io::{self, Read as _};
use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::os::windows::io::{AsRawHandle as _, IntoRawHandle as _};
use std::path::Path;

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::NtCreateDirectoryObject;
use windows::Win32::Foundation::{
    HANDLE, OBJ_CASE_INSENSITIVE, STATUS_OBJECT_NAME_COLLISION, UNICODE_STRING,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED, FILE_SHARE_DELETE, FILE_SHARE_READ,
    GETFINALPATHNAMEBYHANDLE_FLAGS, GetFinalPathNameByHandleW, GetLongPathNameW, VOLUME_NAME_DOS,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use super::{GitBashOs, Made, Ready, Runtime, prepare};
use crate::windows::acl::object::Handle;
use crate::windows::admin_copy::os as admin_os;
use crate::windows::git_tree::{RECORD_SUBKEY, checked_chain};
use crate::windows::record;
use crate::windows::runner::LaunchMessage;
use crate::windows::runner::env::GitEnv;
use crate::windows::runner::paths::WinPath;
use crate::windows::runner::stub::STUB;
use crate::windows::sids::Sid;
use crate::windows::token::os::LocalDescriptor;

/// The record key under `HKLM`.
const RECORD_KEY: &str = r"SOFTWARE\Clauseo\Sandbox";
/// `FILE_ATTRIBUTE_REPARSE_POINT`.
const REPARSE_POINT: u32 = 0x400;
/// The most of `msys-2.0.dll` read: it is about 3.5 MB.
const RUNTIME_CAP: u64 = 64 * 1024 * 1024;
/// The most of a tree's marker read.
const MARKER_CAP: u64 = 4096;
/// What a file opened only to read shares: reading and deleting.
const SHARE_ALL: u32 = FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0;
/// `DIRECTORY_QUERY`: all a held folder needs.
const DIRECTORY_QUERY: u32 = 0x0001;

/// The run's Git Bash: its tree, with its held files and msys's folders, held until this drops.
pub(crate) struct GitBash {
    /// What the agent's environment is made with.
    pub(crate) env: GitEnv,
    /// The tree's held files and msys's folders, open for the run.
    _held: Vec<Handle>,
}

/// The run's Git Bash, or none with its reason said on one line through `say`. The lawyer's
/// Git roots, which the agent's `PATH` never names, are found either way, with each `PATH`
/// entry's long form.
pub(crate) fn for_run(message: &LaunchMessage, desktop: &Sid, say: fn(&str)) -> GitBash {
    let lawyer_git = lawyer_git_roots();
    let long_names = long_names(message);
    match prepare(
        &mut WindowsGitBash,
        &message.pool_user,
        &message.pool_name,
        desktop,
    ) {
        Ok(Ready { tree, held }) => GitBash {
            env: GitEnv {
                tree: Some(tree),
                stub: None,
                off: None,
                lawyer_git,
                long_names,
            },
            _held: held,
        },
        Err(off) => {
            say(&format!(
                "clauseo-sandbox-run: Git Bash is off for this run: {off}"
            ));
            let stub = stub();
            if stub.is_none() {
                say(
                    "clauseo-sandbox-run: the Git Bash stub isn't there, so Claude Code may look \
                     for another Git Bash, which fails under the sandbox user",
                );
            }
            GitBash {
                env: GitEnv {
                    tree: None,
                    stub,
                    off: Some(off.to_string()),
                    lawyer_git,
                    long_names,
                },
                _held: Vec::new(),
            }
        }
    }
}

/// The stub (`stub::STUB` in the admin folder, once its chain passes its check), when it is a
/// regular file there.
fn stub() -> Option<WinPath> {
    let admin = admin_os::admin_dir_checked().ok()?;
    let stub = STUB.iter().fold(admin, |path, part| path.join(part));
    open_plain(stub.to_str()?, SHARE_ALL).ok()?;
    WinPath::parse(stub.to_str()?)
}

/// The per-machine Git installs the lawyer's `PATH` may name: `HKLM\SOFTWARE\GitForWindows`'s
/// `InstallPath`, `%ProgramFiles%\Git` and `%ProgramFiles(x86)%\Git`, each also in its long
/// form. A per-user one lies in the lawyer's profile, which `env::agent_env` already keeps out.
fn lawyer_git_roots() -> Vec<WinPath> {
    let mut roots = Vec::new();
    if let Ok(Some(path)) = record::read_string(w!(r"SOFTWARE\GitForWindows"), w!("InstallPath")) {
        roots.push(path);
    }
    for folder in [admin_os::program_files(), admin_os::program_files_x86()]
        .into_iter()
        .flatten()
    {
        roots.extend(folder.to_str().map(|folder| format!(r"{folder}\Git")));
    }
    let long: Vec<String> = roots.iter().filter_map(|root| long_path(root)).collect();
    roots.extend(long);
    roots
        .iter()
        .filter_map(|root| WinPath::parse(root))
        .collect()
}

/// Each entry of the launch's `PATH` whose long form (`GetLongPathNameW`, where it exists)
/// differs, with that form: so `C:\PROGRA~1\Git\cmd` is seen in `C:\Program Files\Git`.
fn long_names(message: &LaunchMessage) -> Vec<(String, String)> {
    message
        .env
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .flat_map(|(_, value)| value.split(';'))
        .filter_map(|entry| {
            let long = long_path(entry.trim())?;
            (long != entry).then(|| (entry.to_owned(), long))
        })
        .collect()
}

/// `path`'s long form, when it exists; `None` otherwise or when it is the same.
fn long_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let wide = HSTRING::from(path);
    let mut buffer = vec![0_u16; 512];
    for _ in 0..3 {
        // SAFETY: `wide` is NUL-terminated and outlives the call; `buffer` is live and writable.
        let len = unsafe { GetLongPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buffer)) };
        let len = usize::try_from(len).ok().filter(|len| *len > 0)?;
        if len < buffer.len() {
            let long = String::from_utf16(buffer.get(..len)?).ok()?;
            return (long != path).then_some(long);
        }
        buffer.resize(len + 1, 0);
    }
    None
}

/// The machine, as the desktop user.
struct WindowsGitBash;

impl GitBashOs for WindowsGitBash {
    type Held = Handle;

    fn record(&mut self, pool_user: &Sid) -> Result<Option<String>, String> {
        let subkey = HSTRING::from(format!(r"{RECORD_KEY}\{RECORD_SUBKEY}"));
        let value = HSTRING::from(pool_user.as_str());
        record::read_string(PCWSTR(subkey.as_ptr()), PCWSTR(value.as_ptr()))
            .map_err(|error| error.to_string())
    }

    fn admin_dir(&mut self) -> Result<WinPath, String> {
        let dir = admin_os::admin_dir_checked()
            .map_err(|blocked| format!("its chain was refused: {blocked:?}"))?;
        dir.to_str()
            .and_then(WinPath::parse)
            .ok_or_else(|| "it isn't a full local path".to_owned())
    }

    fn group(&mut self) -> Result<Sid, String> {
        record::sandbox_group_sid()
            .map_err(|error| error.to_string())?
            .and_then(|text| Sid::parse(&text).ok())
            .ok_or_else(|| "the sandbox users' group isn't recorded".to_owned())
    }

    fn check_levels(&mut self, admin: &WinPath, levels: &[(String, String)]) -> Result<(), String> {
        checked_chain(Path::new(&admin.display()), levels)
    }

    fn marker(&mut self, path: &WinPath) -> Result<String, String> {
        let mut file =
            open_plain(&path.display(), SHARE_ALL).map_err(|e| said("its marker", &e))?;
        let mut text = String::new();
        (&mut file)
            .take(MARKER_CAP)
            .read_to_string(&mut text)
            .map_err(|e| said("its marker", &e))?;
        Ok(text)
    }

    fn runtime(&mut self, dll: &WinPath) -> Result<Runtime, String> {
        let mut file = open_plain(&dll.display(), SHARE_ALL).map_err(|e| said("opening it", &e))?;
        let final_path = final_path(&file).map_err(|e| said("its final path", &e))?;
        let mut image = Vec::new();
        (&mut file)
            .take(RUNTIME_CAP)
            .read_to_end(&mut image)
            .map_err(|e| said("reading it", &e))?;
        Ok(Runtime { final_path, image })
    }

    fn hold(&mut self, file: &WinPath) -> Result<Handle, String> {
        let name = file.parts().last().map_or_else(String::new, String::clone);
        let opened = open_plain(&file.display(), FILE_SHARE_READ.0)
            .map_err(|e| said(&format!("holding {name}"), &e))?;
        Ok(Handle(HANDLE(opened.into_raw_handle())))
    }

    fn session(&mut self) -> Result<u32, String> {
        let mut session = 0_u32;
        // SAFETY: `session` is a live out-pointer for the call.
        unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &raw mut session) }.map_err(
            |error| format!("ProcessIdToSessionId failed ({})", error.code().0 & 0xFFFF),
        )?;
        Ok(session)
    }

    fn make_dir(&mut self, path: &str, sddl: &str) -> Result<Handle, Made> {
        let descriptor = LocalDescriptor::new(sddl)
            .map_err(|error| Made::Failed(said("its descriptor", &error)))?;
        let name: Vec<u16> = path.encode_utf16().collect();
        let bytes = u16::try_from(name.len() * 2)
            .map_err(|_too_long| Made::Failed("its name is too long".to_owned()))?;
        let unicode = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: PWSTR(name.as_ptr().cast_mut()),
        };
        // No `OBJ_OPENIF`: one already there fails the call, never opens.
        let attributes = OBJECT_ATTRIBUTES {
            Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(0),
            ObjectName: &raw const unicode,
            Attributes: OBJ_CASE_INSENSITIVE,
            SecurityDescriptor: descriptor.raw().cast_const().cast(),
            ..OBJECT_ATTRIBUTES::default()
        };
        let mut handle = HANDLE::default();
        // SAFETY: `unicode` points into `name` and `attributes` at `unicode` and the
        // descriptor, all alive for the call and never written; the handle made is owned by
        // `Handle` from here.
        let status = unsafe {
            NtCreateDirectoryObject(&raw mut handle, DIRECTORY_QUERY, &raw const attributes)
        };
        if status == STATUS_OBJECT_NAME_COLLISION {
            return Err(Made::Exists);
        }
        if status.is_err() {
            return Err(Made::Failed(format!(
                "NtCreateDirectoryObject failed (0x{:08X})",
                status.0
            )));
        }
        Ok(Handle(handle))
    }
}

/// The regular file `path`, opened as itself to read, sharing `share`; a reparse point or a
/// folder is refused.
#[expect(
    clippy::disallowed_methods,
    reason = "opens a tree's file to read it, never to write; std's open is the one with flags"
)]
fn open_plain(path: &str, share: u32) -> io::Result<File> {
    let file = File::options()
        .read(true)
        .share_mode(share)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    let meta = file.metadata()?;
    if meta.file_attributes() & REPARSE_POINT != 0 || !meta.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    Ok(file)
}

/// The final path of what `file` is open on: normalized, drive-letter form (`\\?\C:\…`).
fn final_path(file: &File) -> io::Result<String> {
    let handle = HANDLE(file.as_raw_handle());
    let flags = GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0);
    let mut buffer = vec![0_u16; 512];
    for _ in 0..3 {
        // SAFETY: the handle is `file`'s, open for the call; `buffer` is live and writable.
        let len = unsafe { GetFinalPathNameByHandleW(handle, &mut buffer, flags) };
        let len = usize::try_from(len).unwrap_or(0);
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        if len < buffer.len() {
            return String::from_utf16(buffer.get(..len).unwrap_or_default())
                .map_err(|_not_utf16| io::Error::other("the final path isn't text"));
        }
        buffer.resize(len + 1, 0);
    }
    Err(io::Error::other("the final path kept growing"))
}

/// `what` failed with `error`: its kind and code, never a path.
fn said(what: &str, error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => format!("{what} failed ({code})"),
        None => format!("{what} failed ({})", error.kind()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder made with no `OBJ_OPENIF` refuses a second make under its name while it is
    /// held, and goes with its handle (Windows CI).
    #[test]
    fn a_second_make_of_a_held_object_folder_is_refused() {
        let mut os = WindowsGitBash;
        let session = os.session().unwrap();
        let mut random = [0_u8; 8];
        getrandom::fill(&mut random).unwrap();
        let name = format!("clauseo-test-{}", crate::windows::refresh::hex(&random));
        let path = super::super::object_dirs(session, &name).remove(0);
        let me = crate::windows::record::current_user_sid().unwrap();
        let sddl = format!("O:{me}D:(A;;0xF000F;;;SY)(A;;0xF000F;;;{me})");
        let held = os.make_dir(&path, &sddl).unwrap();
        assert!(matches!(os.make_dir(&path, &sddl), Err(Made::Exists)));
        drop(held);
        assert!(
            os.make_dir(&path, &sddl).is_ok(),
            "gone with its last handle"
        );
    }
}
