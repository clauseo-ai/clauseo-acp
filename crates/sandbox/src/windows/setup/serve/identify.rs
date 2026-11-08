//! The set-up pipe's two checks, with no Windows call: the server's gate on who asked
//! ([`identify`]) and the pipe's DACL ([`pipe_sddl`]), and the app's check of who answers
//! ([`check_server`]) before it sends anything.

use std::fmt;
use std::path::Path;

use crate::windows::pipe::PIPE_RIGHTS;
use crate::windows::setup::owner::MEDIUM_RID;
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;

/// LOCAL SYSTEM.
const SYSTEM: &str = "S-1-5-18";
/// The start of every local or domain account's SID.
const ACCOUNT_PREFIX: &str = "S-1-5-21-";

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
    /// The client's user isn't a local or domain account (`S-1-5-21-...`): a service, LOCAL
    /// SYSTEM, a virtual account.
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
        })
    }
}

/// The user to set up, once `client` passes every rule; otherwise the first rule it fails, in
/// the order [`Refusal`] lists them. `session_user` is the user signed in to the client's
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
    if !client.user.as_str().starts_with(ACCOUNT_PREFIX) {
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
    Ok(client.user.clone())
}

/// The pipe's descriptor, from the record as it is when the server starts: protected; each of
/// the pool's groups refused everything first; SYSTEM everything; per user the owner, per
/// machine every interactive user (`IU`), [`PIPE_RIGHTS`] (read and write, never a new instance,
/// so no client can serve the name); a Medium label with no-write-up and no-read-up. `None`
/// with no admin copy recorded.
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
        "D:P{denies}(A;;GA;;;SY)(A;;0x{PIPE_RIGHTS:x};;;{client})S:(ML;;NWNR;;;ME)"
    ))
}

/// Who answers on the pipe, as the app reads it on the handle it opened, before it writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFacts {
    /// The server's session (`GetNamedPipeServerSessionId`).
    pub session: u32,
    /// The server process's user, from the kernel's process list; `None` when not given.
    pub user: Option<Sid>,
    /// The server process's image as the kernel names it (`\Device\HarddiskVolumeN\...`);
    /// `None` when not given.
    pub image: Option<String>,
}

/// Whether `server` is the set-up task's: session 0, LOCAL SYSTEM, running the admin copy
/// `admin_exe`, whose drive is the device `drive_device` (`QueryDosDeviceW` of its letter).
/// Anything unknown refuses: a pool user can make the pipe's name first, and must get nothing.
pub fn check_server(
    server: &ServerFacts,
    admin_exe: &Path,
    drive_device: &str,
) -> Result<(), String> {
    if server.session != 0 {
        return Err(format!(
            "the pipe's server runs in session {}",
            server.session
        ));
    }
    match &server.user {
        Some(user) if user.as_str() == SYSTEM => {}
        Some(_) => return Err("the pipe's server isn't LOCAL SYSTEM".to_owned()),
        None => return Err("the pipe's server's user can't be read".to_owned()),
    }
    let exe = admin_exe.to_string_lossy();
    let expected = exe
        .get(2..)
        .filter(|_| exe.as_bytes().get(1) == Some(&b':'))
        .map(|rest| format!("{}{rest}", drive_device.trim_end_matches('\\')));
    match (&server.image, expected) {
        (Some(image), Some(expected)) if image.to_lowercase() == expected.to_lowercase() => Ok(()),
        (Some(_), Some(_)) => Err("the pipe's server isn't the admin copy".to_owned()),
        (None, _) => Err("the pipe's server's image can't be read".to_owned()),
        (_, None) => Err("the admin copy's path has no drive".to_owned()),
    }
}
