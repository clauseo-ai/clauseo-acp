//! The set-up pipe's two checks, with no Windows call: the server's gate on who asked
//! ([`identify`]) and the pipe's descriptor ([`pipe_sddl`]), and the app's check of who
//! answers ([`check_server`]) before it sends anything.

use std::fmt;
use std::path::Path;

use crate::windows::pipe::PIPE_RIGHTS;
use crate::windows::setup::owner::MEDIUM_RID;
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;

/// LOCAL SYSTEM.
const SYSTEM: &str = "S-1-5-18";
/// INTERACTIVE: in the token of every console and Remote Desktop logon.
const INTERACTIVE: &str = "S-1-5-4";
/// NETWORK: in the token of a network logon.
const NETWORK: &str = "S-1-5-2";

/// Who connected, as the server reads it: the client's token at identification level and the
/// session the pipe says it runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientFacts {
    /// The token's user.
    pub user: Sid,
    /// The client's session (`GetNamedPipeClientSessionId`), which its process can't change.
    pub session: u32,
    /// Every group in the token, enabled, deny-only or not.
    pub groups: Vec<Sid>,
    /// Whether the token lists restricting SIDs (`TokenRestrictedSids`).
    pub restricted: bool,
    /// The relative id of the token's integrity level.
    pub integrity: u32,
}

/// What the record says about who may be served, read when asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gate {
    /// `AdminCopyOwner`: a user per user, `machine` per machine; `None` with no admin copy.
    pub copy_owner: Option<CopyOwner>,
    /// `PoolSids`.
    pub pool: Vec<Sid>,
    /// The pool's group: `GroupSid`, and the SID of the group named `ClauseoSandboxUsers` when
    /// it differs (one a failed first set-up left).
    pub groups: Vec<Sid>,
}

/// Why a client was refused. Its words name no SID or path: they go to the Event Log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The record holds no admin copy.
    NoAdminCopy,
    /// The client's user is a pool user.
    PoolUser,
    /// The client's token holds the pool's group.
    PoolGroup,
    /// The client runs in session 0, where no one signs in.
    SessionZero,
    /// The client's user isn't a person's account ([`Sid::is_user_account`]: a machine's, an AD
    /// domain's or an Entra ID user's): a service, LOCAL SYSTEM, a virtual account, an
    /// `AppContainer`.
    NotAnAccount,
    /// The client's token is restricted.
    Restricted,
    /// The client's token is below Medium integrity.
    LowIntegrity,
    /// The session's user couldn't be read.
    NoSessionUser,
    /// The client isn't the user signed in to its session.
    NotSessionUser,
    /// A per-user install, and the client isn't the user it was installed for.
    NotTheOwner,
    /// The client's token is a network logon's (it holds `NETWORK`).
    NetworkLogon,
    /// The client's token isn't an interactive logon's (it lacks `INTERACTIVE`).
    NotInteractive,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refusal::NoAdminCopy => "no admin copy is recorded",
            Refusal::SessionZero => "the client runs in session 0",
            Refusal::NotAnAccount => "the client isn't a user account",
            Refusal::PoolUser => "the client is a sandbox user",
            Refusal::PoolGroup => "the client holds the sandbox users' group",
            Refusal::Restricted => "the client's token is restricted",
            Refusal::LowIntegrity => "the client runs below Medium integrity",
            Refusal::NoSessionUser => "the user of the client's session can't be read",
            Refusal::NotSessionUser => "the client isn't its session's user",
            Refusal::NotTheOwner => "this install is another user's",
            Refusal::NetworkLogon => "the client is a network logon",
            Refusal::NotInteractive => "the client isn't an interactive logon",
        })
    }
}

/// The user to set up, once `client` passes every rule; otherwise the first rule it fails, in
/// the order [`Refusal`] lists them. The lawyer is known by token, never by a SID's prefix: the
/// user signed in to the client's session, a person's account of any kind Windows signs in
/// interactively (local, AD, Microsoft-account-linked, Entra ID), in no part of the pool, on an
/// interactive logon that isn't a network one. `session_user` is the user signed in to the client's
/// session, `None` when it couldn't be read. The pool rules come before everything else and
/// read only the record and the token: no descriptor, no DACL and no session decides them, so
/// they hold before the task's deny exists and if the pipe's DACL were ever wrong.
pub fn identify(
    client: &ClientFacts,
    session_user: Option<&Sid>,
    gate: &Gate,
) -> Result<Sid, Refusal> {
    let Some(copy_owner) = &gate.copy_owner else {
        return Err(Refusal::NoAdminCopy);
    };
    if gate.pool.contains(&client.user) {
        return Err(Refusal::PoolUser);
    }
    if client
        .groups
        .iter()
        .any(|group| gate.groups.contains(group))
        || gate.groups.contains(&client.user)
    {
        return Err(Refusal::PoolGroup);
    }
    if client.session == 0 {
        return Err(Refusal::SessionZero);
    }
    if !client.user.is_user_account() {
        return Err(Refusal::NotAnAccount);
    }
    if client.restricted {
        return Err(Refusal::Restricted);
    }
    if client.integrity < MEDIUM_RID {
        return Err(Refusal::LowIntegrity);
    }
    let Some(session_user) = session_user else {
        return Err(Refusal::NoSessionUser);
    };
    if *session_user != client.user {
        return Err(Refusal::NotSessionUser);
    }
    if let CopyOwner::User(owner) = copy_owner
        && *owner != client.user
    {
        return Err(Refusal::NotTheOwner);
    }
    if client.groups.iter().any(|group| group.as_str() == NETWORK) {
        return Err(Refusal::NetworkLogon);
    }
    if !client
        .groups
        .iter()
        .any(|group| group.as_str() == INTERACTIVE)
    {
        return Err(Refusal::NotInteractive);
    }
    Ok(client.user.clone())
}

/// The pipe's descriptor, from the record as it is when the server starts: owned by LOCAL
/// SYSTEM (`O:SY`, the proof [`check_server`] reads: no one but SYSTEM or an administrator can
/// make an object SYSTEM owns); protected; each of the pool's groups refused everything first;
/// SYSTEM everything; per user the owner, per machine every interactive user (`IU`),
/// [`PIPE_RIGHTS`] (read and write, never a new instance, so no client can serve the name); a
/// Medium label with no-write-up and no-read-up. `None` with no admin copy recorded.
pub fn pipe_sddl(gate: &Gate) -> Option<String> {
    let client = match gate.copy_owner.as_ref()? {
        CopyOwner::User(owner) => owner.to_string(),
        CopyOwner::Machine => "IU".to_owned(),
    };
    let denies = gate.groups.iter().fold(String::new(), |mut denies, group| {
        denies.push_str("(D;;GA;;;");
        denies.push_str(group.as_str());
        denies.push(')');
        denies
    });
    Some(format!(
        "O:SYD:P{denies}(A;;GA;;;SY)(A;;0x{PIPE_RIGHTS:x};;;{client})S:(ML;;NWNR;;;ME)"
    ))
}

/// Who answers on the pipe, as the app reads it on the handle it opened, before it writes. A
/// fact is `Err`, with why, when Windows wouldn't give it to this user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFacts {
    /// The pipe's owner (`GetSecurityInfo` on the handle, which [`PIPE_RIGHTS`]' `READ_CONTROL`
    /// allows).
    pub owner: Result<Sid, String>,
    /// The server's session (`GetNamedPipeServerSessionId`).
    pub session: Result<u32, String>,
    /// The server process's user, from the kernel's process list.
    pub user: Result<Sid, String>,
    /// The server process's image as the kernel names it (`\Device\HarddiskVolumeN\...`).
    pub image: Result<String, String>,
}

/// The pipe's owner check, the proof [`check_server`] starts with: `owner` is LOCAL SYSTEM. The
/// app reads it before anything else, so a squatter's pipe is refused on it alone, whatever
/// became of the process that made it.
pub fn check_owner(owner: &Result<Sid, String>) -> Result<(), String> {
    match owner {
        Ok(owner) if owner.as_str() == SYSTEM => Ok(()),
        Ok(_) => Err("the pipe isn't owned by LOCAL SYSTEM".to_owned()),
        Err(why) => Err(format!("the pipe's owner can't be read: {why}")),
    }
}

/// Whether a pipe that failed [`check_owner`] is a pool user's squat, which stopping the pool's
/// processes frees: its owner `owner` is one of `pool` (`PoolSids`) or `groups` (the pool's
/// group). Any other owner, or one that couldn't be read, is not: per machine, stopping the pool
/// for it would end every lawyer's matters and free nothing.
pub fn pool_squat(owner: &Result<Sid, String>, pool: &[Sid], groups: &[Sid]) -> bool {
    owner
        .as_ref()
        .is_ok_and(|owner| pool.contains(owner) || groups.contains(owner))
}

/// Whether `server` is the set-up task's. The proof is the pipe's owner ([`check_owner`]): the
/// server makes the pipe owned by LOCAL SYSTEM ([`pipe_sddl`]), which a pool user can't, and
/// the owner stays with the pipe, so a squatter's pipe passed to another process, or a server
/// id reused after the squatter ended, proves nothing. The rest back it up: session 0, LOCAL
/// SYSTEM, running the admin copy `admin_exe`, whose drive is the device `drive_device`
/// (`QueryDosDeviceW` of its letter). A fact read that differs refuses; a fact Windows wouldn't
/// give this user is set aside on the owner's word, and returned, a line each, for the log.
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
