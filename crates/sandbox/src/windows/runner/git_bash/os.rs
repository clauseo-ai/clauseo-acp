//! [`GitBashOs`] on Windows, as the desktop user: the record, the tree's checks through
//! handles (`git_tree::checked_chain`), the runtime's final path, and msys's folders made with
//! `NtCreateDirectoryObject`; the Git Bash stubs' checks and the outer runner's own copy.
#![expect(
    unsafe_code,
    reason = "FFI to GetFinalPathNameByHandleW, ProcessIdToSessionId, NtCreateDirectoryObject, \
              CopyFileW, MoveFileExW and DeleteFileW"
)]

mod ids;

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
    CopyFileW, DeleteFileW, FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED, FILE_SHARE_DELETE,
    FILE_SHARE_READ, GETFINALPATHNAMEBYHANDLE_FLAGS, GetFinalPathNameByHandleW,
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, VOLUME_NAME_DOS,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use self::ids::WindowsIds;
use super::off::{NO_STUB, off_stub};
use super::path_ids::{FileIds as _, dropped_entries};
use super::{GitBashOs, Made, Ready, Runtime, prepare};
use crate::windows::acl::object::Handle;
use crate::windows::admin_copy::os as admin_os;
use crate::windows::git_tree::{RECORD_SUBKEY, checked_chain, checked_file};
use crate::windows::runner::LaunchMessage;
use crate::windows::runner::env::{Bash, GitEnv};
use crate::windows::runner::paths::WinPath;
use crate::windows::sids::Sid;
use crate::windows::token::os::LocalDescriptor;
use crate::windows::{private_dir, record};

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

/// The run's Git Bash, or a stub (`off.rs`) with why it is off said on one line through `say`;
/// `Err` with the plain line of a launch refused when no stub passes. `exe` is this runner's
/// own exe, `data_dir` the launch's data folder. The `PATH` entries the agent's leaves out are
/// found either way (`path_ids.rs`).
pub(crate) fn for_run(
    message: &LaunchMessage,
    desktop: &Sid,
    exe: &Path,
    data_dir: &Path,
    say: fn(&str),
) -> Result<GitBash, String> {
    let spec_path = message
        .env
        .iter()
        .rev()
        .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
        .map_or("", |(_, value)| value.as_str());
    let dropped = dropped_entries(spec_path, &lawyer_git_roots(), &mut WindowsIds);
    let mut os = WindowsGitBash { exe };
    let off = match prepare(&mut os, &message.pool_user, &message.pool_name, desktop) {
        Ok(Ready { tree, held }) => {
            return Ok(GitBash {
                env: GitEnv {
                    bash: Bash::Tree(tree),
                    dropped,
                },
                _held: held,
            });
        }
        Err(off) => off,
    };
    say(&format!(
        "clauseo-sandbox-run: Git Bash is off for this run: {off}"
    ));
    let data_dir = data_dir.to_str().unwrap_or_default();
    match off_stub(&mut os, desktop, data_dir) {
        Ok((stub, machine)) => {
            if let Some(why) = machine {
                say(&format!(
                    "clauseo-sandbox-run: the machine's Git Bash stub was passed over ({why}); \
                     the runner's own copy stands in"
                ));
            }
            Ok(GitBash {
                env: GitEnv {
                    bash: Bash::Off {
                        stub,
                        reason: off.to_string(),
                    },
                    dropped,
                },
                _held: Vec::new(),
            })
        }
        Err(why) => Err(format!("{NO_STUB} ({why})")),
    }
}

/// The per-machine Git installs the lawyer's `PATH` may name: `HKLM\SOFTWARE\GitForWindows`'s
/// `InstallPath`, `%ProgramFiles%\Git` and `%ProgramFiles(x86)%\Git`, compared by file ID. A
/// per-user one lies in the lawyer's profile, which `env::agent_env` already keeps out.
fn lawyer_git_roots() -> Vec<String> {
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
    roots
}

/// The machine, as the desktop user; `exe` is this runner's own exe.
struct WindowsGitBash<'a> {
    exe: &'a Path,
}

impl GitBashOs for WindowsGitBash<'_> {
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

    fn stub_security(
        &mut self,
        admin: &WinPath,
        levels: &[(String, String)],
        name: &str,
    ) -> Result<Vec<u8>, String> {
        checked_file(Path::new(&admin.display()), levels, name)
    }

    fn local_app_data(&mut self) -> Result<String, String> {
        let folder = admin_os::local_app_data().map_err(|e| said("finding LocalAppData", &e))?;
        let text = folder
            .to_str()
            .ok_or_else(|| "LocalAppData isn't text".to_owned())?;
        let drive = WinPath::parse(text).and_then(|path| path.drive().map(str::to_owned));
        match drive {
            Some(drive) if !WindowsIds.remote(&drive) => Ok(text.to_owned()),
            _ => Err("LocalAppData is on the network".to_owned()),
        }
    }

    fn secure_dir(&mut self, dir: &WinPath, sddl: &str) -> Result<(), String> {
        let path = Path::new(&dir.display()).to_path_buf();
        let made = private_dir::create_with(&path, sddl).map_err(|e| said("making it", &e))?;
        if !made {
            private_dir::secure_existing(&path, sddl).map_err(|e| said("securing it", &e))?;
        }
        Ok(())
    }

    fn runner_hash(&mut self) -> Result<String, String> {
        admin_os::sha256(self.exe).map_err(|e| said("hashing the runner", &e))
    }

    fn file_hash(&mut self, file: &WinPath) -> Result<Option<String>, String> {
        // Opened as itself: a link or a folder there is refused, never followed.
        match admin_os::sha256(Path::new(&file.display())) {
            Ok(hash) => Ok(Some(hash)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(said("hashing the copy", &error)),
        }
    }

    fn copy_runner(&mut self, to: &WinPath) -> Result<(), String> {
        let mut random = [0_u8; 8];
        getrandom::fill(&mut random).map_err(|e| format!("random failed ({e})"))?;
        let temp = HSTRING::from(format!(
            "{}.{}.tmp",
            to.display(),
            crate::windows::refresh::hex(&random)
        ));
        let from = HSTRING::from(self.exe.as_os_str());
        let target = HSTRING::from(to.display());
        // SAFETY: each string is NUL-terminated and outlives its call.
        unsafe { CopyFileW(&from, &temp, true) }
            .map_err(|e| format!("copying the runner failed ({})", e.code().0 & 0xFFFF))?;
        let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
        // SAFETY: as above.
        if let Err(error) = unsafe { MoveFileExW(&temp, &target, flags) } {
            // SAFETY: as above.
            drop(unsafe { DeleteFileW(&temp) });
            return Err(format!(
                "putting the copy in place failed ({})",
                error.code().0 & 0xFFFF
            ));
        }
        Ok(())
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
        let mut os = WindowsGitBash { exe: Path::new("") };
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
