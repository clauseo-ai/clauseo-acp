//! The check of the watcher's pipe server, with no Windows call: the facts the client reads on
//! the handle it opened, before it reads a byte, and what they must be.
//!
//! The proof is the pipe's owner: the watcher makes the pipe owned by LOCAL SYSTEM
//! (`protocol::pipe_sddl`), which a pool user can't, and the owner stays with the pipe, so a
//! squatter's pipe handed to another process, or a server id reused after the squatter ended,
//! proves nothing. The rest backs it up: the server runs in session 0, as LOCAL SYSTEM, from the
//! admin copy. A fact read that differs refuses; a fact Windows won't give this user is set aside
//! on the owner's word and logged. This is the check `SandboxSetUp`'s client makes (#733,
//! `setup::serve::check_server`); once both are on one branch, they become one function.

use std::path::Path;

use crate::windows::sids::Sid;
use crate::windows::token::SYSTEM;

/// Who serves the pipe, as the client reads it. A fact is `Err`, with why, when Windows
/// wouldn't give it to this user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFacts {
    /// The pipe's owner (`GetSecurityInfo` on the handle, which `FILE_GENERIC_READ`'s
    /// `READ_CONTROL` allows).
    pub owner: Result<Sid, String>,
    /// The server's session (`GetNamedPipeServerSessionId`).
    pub session: Result<u32, String>,
    /// The server process's user (`GetNamedPipeServerProcessId`, then the kernel's process
    /// list).
    pub user: Result<Sid, String>,
    /// The server process's image as the kernel names it (`\Device\HarddiskVolumeN\...`).
    pub image: Result<String, String>,
}

/// The owner check, read first: `owner` is LOCAL SYSTEM.
pub fn check_owner(owner: &Result<Sid, String>) -> Result<(), String> {
    match owner {
        Ok(owner) if owner.as_str() == SYSTEM => Ok(()),
        Ok(_) => Err("the pipe isn't owned by LOCAL SYSTEM".to_owned()),
        Err(why) => Err(format!("the pipe's owner can't be read: {why}")),
    }
}

/// Whether `server` is the watcher: [`check_owner`], then session 0, LOCAL SYSTEM, and the
/// admin copy `admin_exe`, whose drive is the device `drive_device` (`QueryDosDeviceW` of its
/// letter). `Ok` holds the facts set aside, a line each, for the log.
pub fn check_server(
    server: &ServerFacts,
    admin_exe: &Path,
    drive_device: &str,
) -> Result<Vec<String>, String> {
    check_owner(&server.owner)?;
    let mut unread = Vec::new();
    match &server.session {
        Ok(0) => {}
        Ok(session) => return Err(format!("the pipe's server runs in session {session}")),
        Err(why) => unread.push(format!("its session: {why}")),
    }
    match &server.user {
        Ok(user) if user.as_str() == SYSTEM => {}
        Ok(_) => return Err("the pipe's server isn't LOCAL SYSTEM".to_owned()),
        Err(why) => unread.push(format!("its user: {why}")),
    }
    let exe = admin_exe.to_string_lossy();
    let expected = exe
        .get(2..)
        .filter(|_| exe.as_bytes().get(1) == Some(&b':'))
        .map(|rest| format!("{}{rest}", drive_device.trim_end_matches('\\')))
        .ok_or("the admin copy's path has no drive")?;
    match &server.image {
        Ok(image) if image.to_lowercase() == expected.to_lowercase() => {}
        Ok(_) => return Err("the pipe's server isn't the admin copy".to_owned()),
        Err(why) => unread.push(format!("its image: {why}")),
    }
    Ok(unread)
}
