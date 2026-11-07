//! The agent's token under design (B) (S15 `PR5`, D9, D11): what the runner, logged on as the
//! matter's pool user, makes from its own token before it starts the agent.
//!
//! - **Restricting SIDs:** the pool user, `RESTRICTED`, `Users` and `Everyone`, and the matter's
//!   SID only when a launch asks for it (0049: off by default; spike B found it adds nothing).
//!   `Everyone` stays (D11): without it codex's `bcrypt.dll` can't open `\Device\KsecDD` and
//!   nsiproxy refuses name lookups (`s15-spikes-everyone.md`). `Users` stays (PR0): System32 DLLs
//!   outside `KnownDLLs` grant `Users` alone.
//! - **The logon SID deny-only.** `CreateProcessWithLogonW` hands the runner the desktop user's
//!   own logon SID, so it is no boundary between launches; deny-only, it opens nothing the
//!   lawyer's processes or another runner's agent hold through it (PR0 (a) and (d)).
//! - **Low integrity** (`S-1-16-4096`): the agent writes nothing labelled higher; the matter,
//!   state and notes folders carry the Low label PR3 writes.
//! - **Default DACL:** the pool user and `SYSTEM` with full access, and the desktop user with
//!   `SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE` (`0x101001`) only, so
//!   the host can wait on, query and end the agent's processes (R5) and nothing else; no logon
//!   SID, so another matter's agent can't `OpenProcess` this one (PR0 (d), spike B). The pool
//!   user keeps `GENERIC_ALL` with nothing denied: a console process with no console (the
//!   agent starts `CREATE_NO_WINDOW`) makes its `\Device\ConDrv\Server` with `GENERIC_ALL`
//!   under this DACL, and dies at start with `STATUS_DLL_INIT_FAILED` if any right is denied.
//! - `DISABLE_MAX_PRIVILEGE`, as the traverse spike's (B) row ran it
//!   (`s15-spikes-traverse/s15.c:183`): every privilege but `SeChangeNotifyPrivilege` goes, and
//!   that one must stay (§3, "Bypass traverse stays in the token").
//!
//! The spec is built from its inputs with no Windows call ([`TokenSpec::for_pool`]), so its
//! tests run on every OS; `os` (Windows) applies it to the runner's own token.

use std::fmt;

use super::sids::Sid;

#[cfg(windows)]
pub(in crate::windows) mod os;

/// `Everyone` (`S-1-1-0`).
pub const EVERYONE: &str = "S-1-1-0";
/// `NT AUTHORITY\RESTRICTED` (`S-1-5-12`).
pub const RESTRICTED: &str = "S-1-5-12";
/// `BUILTIN\Users` (`S-1-5-32-545`).
pub const USERS: &str = "S-1-5-32-545";
/// `NT AUTHORITY\SYSTEM` (`S-1-5-18`).
pub const SYSTEM: &str = "S-1-5-18";
/// The Low mandatory level (`S-1-16-4096`).
pub const LOW_INTEGRITY: &str = "S-1-16-4096";
/// What the desktop user may do to the agent's processes and threads: `SYNCHRONIZE`
/// (`0x100000`), `PROCESS_QUERY_LIMITED_INFORMATION` (`0x1000`) and `PROCESS_TERMINATE` (`0x1`).
pub const DESKTOP_USER_RIGHTS: u32 = 0x0010_1001;

/// Why a token spec couldn't be built from its inputs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenSpecError {
    /// The pool user is the desktop user: the agent would run as the lawyer.
    #[error("the pool user is the desktop user")]
    PoolIsDesktopUser,
    /// A SID that must name one account is a well-known group or service instead.
    #[error("{role} is a well-known SID ({sid}), not an account")]
    NotAnAccount {
        /// Which input: the pool user, the desktop user or the matter SID.
        role: &'static str,
        /// The SID given.
        sid: Sid,
    },
    /// The logon SID isn't of the form `S-1-5-5-X-Y`.
    #[error("{0} is not a logon SID")]
    NotALogonSid(Sid),
    /// One of the well-known SIDs didn't parse (it can't: they are canonical).
    #[error("{0} didn't parse")]
    WellKnown(&'static str),
}

/// The (B) token's parts, built from the launch's SIDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSpec {
    /// The restricting SIDs, in this order: the pool user, `RESTRICTED`, `Users`, `Everyone`,
    /// then the matter's SID when the launch asked for one.
    pub restricting: Vec<Sid>,
    /// The SIDs made deny-only: the logon SID alone.
    pub deny_only: Vec<Sid>,
    /// The token's integrity level: Low.
    pub integrity: Sid,
    /// The default DACL, as SDDL: what every object the agent makes with no descriptor gets.
    pub default_dacl: String,
    /// The agent token object's own DACL, set explicitly ([`agent_token_sddl`]), so it never
    /// depends on what the inner runner's (shut) token passes on.
    pub token_dacl: String,
}

impl TokenSpec {
    /// The spec for an agent running as `pool_user`, whose runner's token holds `logon` (the
    /// desktop user's logon SID), started for `desktop_user`; `matter` adds the matter's SID as a
    /// restricting SID.
    pub fn for_pool(
        pool_user: &Sid,
        logon: &Sid,
        desktop_user: &Sid,
        matter: Option<&Sid>,
    ) -> Result<TokenSpec, TokenSpecError> {
        account("the pool user", pool_user)?;
        account("the desktop user", desktop_user)?;
        if let Some(matter) = matter {
            account("the matter SID", matter)?;
        }
        if pool_user == desktop_user {
            return Err(TokenSpecError::PoolIsDesktopUser);
        }
        if !is_logon_sid(logon) {
            return Err(TokenSpecError::NotALogonSid(logon.clone()));
        }
        let mut restricting = vec![
            pool_user.clone(),
            known(RESTRICTED)?,
            known(USERS)?,
            known(EVERYONE)?,
        ];
        restricting.extend(matter.cloned());
        Ok(TokenSpec {
            restricting,
            deny_only: vec![logon.clone()],
            integrity: known(LOW_INTEGRITY)?,
            default_dacl: default_dacl(pool_user, desktop_user),
            token_dacl: agent_token_sddl(pool_user, desktop_user),
        })
    }
}

impl fmt::Display for TokenSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let restricting: Vec<&str> = self.restricting.iter().map(Sid::as_str).collect();
        let deny: Vec<&str> = self.deny_only.iter().map(Sid::as_str).collect();
        write!(
            f,
            "restricting {}; deny-only {}; integrity {}; default DACL {}",
            restricting.join(","),
            deny.join(","),
            self.integrity,
            self.default_dacl
        )
    }
}

/// The agent's default DACL: `D:(A;;GA;;;<pool user>)(A;;GA;;;SY)(A;;0x101001;;;<desktop
/// user>)`, the spike's `pool` mode (`s15-spikes-userpool/s15.c:182`).
///
/// The pool user's `GENERIC_ALL` must stay whole. The agent starts `CREATE_NO_WINDOW`, so
/// `kernelbase` makes its console's `\Device\ConDrv\Server` with `GENERIC_ALL` as the agent
/// (`DeviceHandle::CreateServerHandle` in microsoft/terminal), under a descriptor from this DACL;
/// a deny of `WRITE_DAC` and `WRITE_OWNER` failed that open, and every agent ended at start with
/// `STATUS_DLL_INIT_FAILED` (the S15 PR5 harness, `b242f2ee9`). Such a deny would hold nothing
/// back anyway: a process sets its own DACL through its pseudo-handle, whose rights the kernel
/// grants at creation whatever the DACL (`token::os::shut_self` does just that).
pub fn default_dacl(pool_user: &Sid, desktop_user: &Sid) -> String {
    format!("D:(A;;GA;;;{pool_user})(A;;GA;;;SY)(A;;0x{DESKTOP_USER_RIGHTS:x};;;{desktop_user})")
}

/// The agent token object's DACL: the pool user (the agent reads its own token) and `SYSTEM` with
/// full access, the desktop user `TOKEN_QUERY`.
pub fn agent_token_sddl(pool_user: &Sid, desktop_user: &Sid) -> String {
    format!("D:P(A;;GA;;;{pool_user})(A;;GA;;;SY)(A;;0x8;;;{desktop_user})")
}

/// The runners' process DACL, the inner's and the end sweeper's (set by the outer while each is
/// suspended, and by the inner itself): `SYSTEM` full, the desktop user and the desktop user's
/// logon SID wait, query and end, `OWNER RIGHTS` held to `READ_CONTROL`, and no pool user, so
/// the agent (the same user) opens nothing of it.
///
/// The logon SID's entry is for the pool user's sweeps (`runner/sweep.rs`):
/// `CreateProcessWithLogonW` gives the inner and the end sweeper the desktop user's logon SID,
/// enabled, so a next inner's first act can end a sweeper still running and a sweeper can end an
/// inner still alive. The agent holds that SID deny-only (`TokenSpec::deny_only`), so an allow
/// entry never matches it.
/// A pool-user process a service started outside the job runs in the pool user's own logon
/// session and doesn't hold it. Other matters' runners started from the same logon do, and can
/// open this one, but their sweeps only end their own pool user's processes. A sweeper from
/// before the desktop user signed out and in again holds the old logon SID, so a launch during
/// its 60 s fails `RunExit::Sweep` and a retry works (accepted).
pub fn runner_process_sddl(desktop_user: &Sid, logon: &Sid) -> String {
    format!(
        "D:P(A;;GA;;;SY)(A;;0x{DESKTOP_USER_RIGHTS:x};;;{desktop_user})\
         (A;;0x{DESKTOP_USER_RIGHTS:x};;;{logon})(A;;RC;;;OW)"
    )
}

/// The inner runner's threads' DACL: `SYSTEM` full, the desktop user `SYNCHRONIZE`, `OWNER
/// RIGHTS` held to `READ_CONTROL`.
pub fn runner_thread_sddl(desktop_user: &Sid) -> String {
    format!("D:P(A;;GA;;;SY)(A;;0x100000;;;{desktop_user})(A;;RC;;;OW)")
}

/// The inner runner's primary token's DACL: `SYSTEM` full, the desktop user `TOKEN_QUERY`,
/// `OWNER RIGHTS` held to `READ_CONTROL`.
pub fn runner_token_sddl(desktop_user: &Sid) -> String {
    format!("D:P(A;;GA;;;SY)(A;;0x8;;;{desktop_user})(A;;RC;;;OW)")
}

/// The inner runner's default DACL once shut: what its later objects (a thread the system starts
/// in it) get: [`runner_thread_sddl`]'s entries, `SYSTEM` full, the desktop user `SYNCHRONIZE`,
/// `OWNER RIGHTS` held to `READ_CONTROL` (so the pool user, their owner, keeps no implied
/// `WRITE_DAC`), and no pool user.
pub fn runner_default_dacl(desktop_user: &Sid) -> String {
    format!("D:(A;;GA;;;SY)(A;;0x100000;;;{desktop_user})(A;;RC;;;OW)")
}

/// The inner runner's primary token's DACL from its first instruction until the launch arrives:
/// `SYSTEM` full and `OWNER RIGHTS` held to `READ_CONTROL`, nobody else. [`runner_token_sddl`]
/// adds the desktop user's `TOKEN_QUERY` once the launch names them.
pub const RUNNER_TOKEN_FIRST_SDDL: &str = "D:P(A;;GA;;;SY)(A;;RC;;;OW)";

/// The private desktop's descriptor: `SYSTEM` and the pool user full (and the matter SID when
/// the launch restricts with one), labelled Low so the Low agent may draw on it; no logon SID and
/// no desktop user, so neither the lawyer's processes nor another runner's agent can open it
/// (`s15-spikes-traverse/s15.c:350-356`, the `pm` mode).
pub fn desktop_sddl(pool_user: &Sid, matter: Option<&Sid>) -> String {
    let matter = matter
        .map(|sid| format!("(A;;GA;;;{sid})"))
        .unwrap_or_default();
    format!("D:(A;;GA;;;SY)(A;;GA;;;{pool_user}){matter}S:(ML;;NW;;;LW)")
}

/// The agent's window station's descriptor: as [`desktop_sddl`], `SYSTEM` and the pool user
/// (and the matter SID when the launch restricts with one), labelled Low.
pub fn station_sddl(pool_user: &Sid, matter: Option<&Sid>) -> String {
    desktop_sddl(pool_user, matter)
}

/// Whether `sid` is a logon SID: `S-1-5-5-X-Y`.
pub fn is_logon_sid(sid: &Sid) -> bool {
    let parts: Vec<&str> = sid.as_str().split('-').collect();
    parts.len() == 6 && parts.get(..4) == Some(&["S", "1", "5", "5"][..])
}

/// Refuses a SID that can't be one account: anything outside `S-1-5-21-…` (a machine or domain
/// account), which leaves out every well-known group and service SID.
fn account(role: &'static str, sid: &Sid) -> Result<(), TokenSpecError> {
    let parts: Vec<&str> = sid.as_str().split('-').collect();
    // S-1-5-21-a-b-c-rid: an account of a machine or a domain.
    if parts.len() == 8 && parts.get(..4) == Some(&["S", "1", "5", "21"][..]) {
        return Ok(());
    }
    Err(TokenSpecError::NotAnAccount {
        role,
        sid: sid.clone(),
    })
}

/// One of this file's well-known SIDs. They are canonical, so the error never comes.
fn known(text: &'static str) -> Result<Sid, TokenSpecError> {
    Sid::parse(text).map_err(|_canonical| TokenSpecError::WellKnown(text))
}

#[cfg(test)]
mod tests;

// Two attributes, not `all(windows, test)`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
