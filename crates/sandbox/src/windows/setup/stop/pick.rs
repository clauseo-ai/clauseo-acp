//! Which processes a snapshot holds for the stop ([`pick`]): each process's user as the kernel's
//! process list, its token and its owner give it, and the rule for a process none of them names
//! ([`kernel_made`]). No Windows call: `windows_stopper.rs` reads, the tests fake.

use crate::windows::sids::Sid;

/// The most image names the stop's log line lists ([`image_names`]).
const NAMES_SHOWN: usize = 5;

/// The idle process's pid.
const IDLE_PID: u32 = 0;
/// The System process's pid.
const SYSTEM_PID: u32 = 4;
/// The session services run in, which no interactive logon gets.
const SERVICES_SESSION: u32 = 0;

/// One process as a snapshot saw it: its user as each source gave it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Process {
    /// Its process id.
    pub pid: u32,
    /// Its user as the kernel's process list (`WTSEnumerateProcessesExW`) gave it; `None` when
    /// the list named no SID for it (as for a process whose token's DACL shuts out SYSTEM), or
    /// didn't list it.
    pub listed: Option<Sid>,
    /// Its token's user, read through the handle; `None` when the token couldn't be read.
    pub token: Option<Sid>,
    /// Its process object's owner, read when the token couldn't be; `None` when not read.
    /// A standard user can't make another account the owner of a process, but the owner is the
    /// process's creator, which needn't be its user.
    pub owner: Option<Sid>,
    /// It was opened with `PROCESS_TERMINATE`.
    pub can_stop: bool,
    /// The kernel's process list couldn't be read for this snapshot, so `listed` is `None`
    /// for want of a list, not because the list named no one.
    pub list_failed: bool,
    /// Its session, as the kernel's process list gave it; `None` when the list didn't list it.
    pub session: Option<u32>,
    /// Its image, read when neither the token nor the owner could be.
    pub image: Image,
    /// It is one of WSL's ([`wsl_image`]): read for every process opened, whoever its user.
    pub wsl: bool,
}

/// A process's image, as `windows_stopper.rs` read it through a query-limited handle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Image {
    /// Not read: no handle, or the token or the owner was read instead.
    #[default]
    Unread,
    /// Its Win32 path (`QueryFullProcessImageNameW`).
    Path(String),
    /// It has no image file and isn't a subsystem process: a minimal process (System, Registry,
    /// Memory Compression, Secure System), which only the kernel makes.
    Missing,
    /// It has an image, but its path couldn't be read as a Win32 path; or it has none, and
    /// whether it is a subsystem process couldn't be read.
    Failed,
    /// It has no image file, but is a subsystem process: a WSL 1 (pico) process, which a pool
    /// user's scheduled task could start in session 0.
    Subsystem,
}

impl Image {
    /// Its file name for the log: a path's last component, never a folder; or a fixed word
    /// (`no image`, `subsystem`, `unread`) when there is no path.
    pub fn file_name(&self) -> String {
        match self {
            Image::Path(path) => path
                .rsplit(['\\', '/'])
                .next()
                .filter(|name| !name.is_empty())
                .unwrap_or("unread")
                .to_owned(),
            Image::Missing => "no image".to_owned(),
            Image::Subsystem => "subsystem".to_owned(),
            Image::Unread | Image::Failed => "unread".to_owned(),
        }
    }
}

/// WSL's processes' file names: what a pool user who started WSL runs as itself.
const WSL_IMAGES: [&str; 4] = ["wsl.exe", "wslhost.exe", "wslrelay.exe", "wslg.exe"];

/// Whether `image` is one of WSL's: a file named as one of [`WSL_IMAGES`] (any case, any
/// folder), or a subsystem (WSL 1 pico) process.
pub fn wsl_image(image: &Image) -> bool {
    match image {
        Image::Subsystem => true,
        Image::Path(_) => {
            let name = image.file_name();
            WSL_IMAGES.iter().any(|wsl| wsl.eq_ignore_ascii_case(&name))
        }
        Image::Unread | Image::Missing | Image::Failed => false,
    }
}

/// Whether `image` is a minimal process's, which only the kernel makes: no image file, and not
/// a subsystem process ([`Image::Missing`]). Any image at all, however signed or wherever it
/// lies, doesn't count: a pool user's scheduled task can start a Windows-signed System32 binary
/// as a protected process (PPL) in session 0, and SYSTEM can neither name nor terminate it.
pub fn kernel_made(image: &Image) -> bool {
    *image == Image::Missing
}

/// `names` once each, in the order first seen, joined for the log: at most [`NAMES_SHOWN`], then
/// `and N more` for the rest.
pub fn image_names(names: &[String]) -> String {
    let mut once: Vec<&str> = Vec::new();
    for name in names {
        if !once.contains(&name.as_str()) {
            once.push(name);
        }
    }
    let more = once.len().saturating_sub(NAMES_SHOWN);
    let mut shown: Vec<String> = once
        .iter()
        .take(NAMES_SHOWN)
        .map(|n| (*n).to_owned())
        .collect();
    if more > 0 {
        shown.push(format!("and {more} more"));
    }
    shown.join(", ")
}

/// What [`pick`] made of a process's user.
#[derive(Debug, Clone, PartialEq, Eq)]
enum User {
    /// Read: the list's SID, or the token's when the list gave none, or the owner's when
    /// neither gave one and the owner is one of the pool's SIDs.
    Known(Sid),
    /// Neither the list nor the token gave one, and the owner isn't a pool SID (or wasn't
    /// read); or the list and the token disagree.
    Unknown,
    /// Not looked at: this process, pid 0 or 4, or one with no user from any source that can't
    /// be opened to be terminated, even with `SeDebugPrivilege`, that the list puts in session
    /// 0, and that has no image file and isn't a subsystem process ([`kernel_made`]). A pool
    /// user's process runs in an interactive session, or in session 0 from its scheduled task,
    /// but always from an image file (or as a subsystem process).
    Exempt,
}

/// `process`'s user, as [`User`] says. The owner may add a process to the pool's, never clear
/// one: it is the process's creator, and an elevated administrator's processes, whoever runs
/// them, are owned by `Administrators`.
fn user_of(process: &Process, pool: &[Sid], own_pid: u32) -> User {
    if [own_pid, IDLE_PID, SYSTEM_PID].contains(&process.pid) {
        return User::Exempt;
    }
    let system = || process.session == Some(SERVICES_SESSION) && kernel_made(&process.image);
    match (&process.listed, &process.token, &process.owner) {
        (Some(listed), Some(token), _) if listed != token => User::Unknown,
        (Some(user), _, _) | (None, Some(user), _) => User::Known(user.clone()),
        (None, None, Some(owner)) if pool.contains(owner) => User::Known(owner.clone()),
        (None, None, None) if !process.can_stop && !process.list_failed && system() => User::Exempt,
        (None, None, _) => User::Unknown,
    }
}

/// What one snapshot holds for the stop.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Picked {
    /// Each process whose user is one of the pool's SIDs, with that user, in the order seen,
    /// each pid once.
    pub stop: Vec<(u32, Sid)>,
    /// The processes whose user couldn't be read ([`User::Unknown`]).
    pub unknown: usize,
    /// Each of those processes' image file name ([`Image::file_name`]), in the order seen.
    pub unknown_images: Vec<String>,
}

/// The processes of `seen` to stop, and those whose user is unknown, as [`user_of`] reads
/// each; SIDs are compared exactly. `own_pid` is this process.
pub fn pick(seen: &[Process], pool: &[Sid], own_pid: u32) -> Picked {
    let mut picked = Picked::default();
    let mut looked: Vec<u32> = Vec::new();
    for process in seen {
        if looked.contains(&process.pid) {
            continue;
        }
        looked.push(process.pid);
        match user_of(process, pool, own_pid) {
            User::Known(user) if pool.contains(&user) => picked.stop.push((process.pid, user)),
            User::Unknown => {
                picked.unknown += 1;
                picked.unknown_images.push(process.image.file_name());
            }
            User::Known(_) | User::Exempt => {}
        }
    }
    picked
}
