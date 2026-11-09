//! The agent's token under design (B) (S15 `PR5`, D9, D11): what the runner, logged on as the
//! matter's pool user, makes from its own token before it starts the agent.
//!
//! - **Restricting SIDs:** the pool user, `RESTRICTED`, `Users` and `Everyone`, and the matter's
//!   SID only when a launch asks for it (0049: off by default; spike B found it adds nothing).
//!   `Everyone` stays (D11): without it codex's `bcrypt.dll` can't open `\Device\KsecDD` and
//!   nsiproxy refuses name lookups (`s15-spikes-everyone.md`). `Users` stays (PR0): System32 DLLs
//!   outside `KnownDLLs` grant `Users` alone.
//! - **The logon SID deny-only.** `CreateProcessWithLogonW` gives the runner's pool logon a
//!   fresh logon SID of its own, made per run, never the desktop user's (measured on the VM, and
//!   on the agent's own station, `runner/station.rs`); the deny-only entry is that one, read
//!   from the runner's own token. Deny-only, it opens nothing the runner itself or another run's
//!   processes hold through it (PR0 (a) and (d)). The runner's token DACL as made names it, not
//!   the desktop user, so the outer reads the suspended inner's user without the token
//!   (`runner/inner_user.rs`), and the inner's first act grants the desktop user `TOKEN_QUERY`
//!   for the outer's full check before its go (`runner/start.rs`).
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
    /// The spec for an agent running as `pool_user`, whose runner's token holds `logon` (its
    /// own logon's fresh logon SID, made per run), started for `desktop_user`; `matter` adds the
    /// matter's SID as a restricting SID.
    pub fn for_pool(
        pool_user: &Sid,
        logon: &Sid,
        desktop_user: &Sid,
        matter: Option<&Sid>,
    ) -> Result<TokenSpec, TokenSpecError> {
        account("the pool user", pool_user)?;
        if !desktop_user.is_user_account() {
            return Err(TokenSpecError::NotAnAccount {
                role: "the desktop user",
                sid: desktop_user.clone(),
            });
        }
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

/// The agent token object's DACL: `D:P(A;;0x20009;;;<pool user>)(A;;GA;;;SY)(A;;0x8;;;<desktop
/// user>)(A;;RC;;;OW)`. The pool user (the agent reads its own token) gets
/// [`AGENT_TOKEN_POOL_RIGHTS`] alone: no `TOKEN_ADJUST_DEFAULT`, so the agent can't shut its
/// default DACL before it asks a service for a process that takes no descriptor of its own (one
/// outside the job the sweep then couldn't end), and no `WRITE_DAC` or `WRITE_OWNER` to give
/// itself more; `OWNER RIGHTS` held to `READ_CONTROL` takes away the owner's implied
/// `WRITE_DAC`. The desktop user gets `TOKEN_QUERY`. The inner runner starts the agent through
/// the handle `CreateRestrictedToken` gave it, whose rights this DACL, set after, doesn't touch.
pub fn agent_token_sddl(pool_user: &Sid, desktop_user: &Sid) -> String {
    format!(
        "D:P(A;;0x{AGENT_TOKEN_POOL_RIGHTS:x};;;{pool_user})(A;;GA;;;SY)(A;;0x8;;;{desktop_user})\
         (A;;RC;;;OW)"
    )
}

/// What the pool user may do to the agent's token: `READ_CONTROL` (`0x20000`), `TOKEN_QUERY`
/// (`0x8`) and `TOKEN_ASSIGN_PRIMARY` (`0x1`).
pub const AGENT_TOKEN_POOL_RIGHTS: u32 = 0x0002_0009;

/// The runners' process descriptor, the inner's and the end sweeper's (set by the outer while
/// each is suspended, and by the inner itself): `SYSTEM` full, the desktop user wait, query and
/// end, `OWNER RIGHTS` held to `READ_CONTROL` (so the pool user, their owner, keeps no implied
/// `WRITE_DAC`), and no pool user, so the agent (the same user) and any other process of the pool
/// user's open nothing of it; and a Medium label with no-write-up, no-read-up and no-execute-up,
/// so a Low process gets nothing of it either, whatever the DACL.
///
/// No runner opens another: a previous session's end sweeper still running when a launch for the
/// same pool user begins is waited for, then ended, by the outer before its logon
/// (`runner/previous.rs`). No logon SID is named: each runner holds its own logon's fresh logon
/// SID (`CreateProcessWithLogonW` gives every child its own, never the desktop user's).
pub fn runner_process_sddl(desktop_user: &Sid) -> String {
    format!(
        "D:P(A;;GA;;;SY)(A;;0x{DESKTOP_USER_RIGHTS:x};;;{desktop_user})(A;;RC;;;OW)\
         S:(ML;;NWNRNX;;;ME)"
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

/// The end sweeper's primary token's DACL from its first instruction: `SYSTEM` full and
/// `OWNER RIGHTS` held to `READ_CONTROL`, nobody else. The inner runner's adds the desktop
/// user's `TOKEN_QUERY` ([`runner_token_first_sddl`]).
pub const RUNNER_TOKEN_FIRST_SDDL: &str = "D:P(A;;GA;;;SY)(A;;RC;;;OW)";

/// `TOKEN_QUERY`: the one right the desktop user gets on the inner runner's token before the
/// launch, for the outer's full check (`runner/outer/steps.rs`). No `TOKEN_DUPLICATE`,
/// `TOKEN_IMPERSONATE`, `TOKEN_ADJUST_*` or `WRITE_DAC`.
pub const RUNNER_TOKEN_QUERY: u32 = 0x0000_0008;

/// The inner runner's primary token's DACL from its first instruction until the launch arrives:
/// [`RUNNER_TOKEN_FIRST_SDDL`]'s entries and `TOKEN_QUERY` for exactly `desktop_user` (from
/// `--inner`), so the outer, as that user, can open the token for its full check after the
/// resume: on the agent's own station the token the Secondary Logon service makes names a fresh
/// logon SID and not the desktop user. [`runner_token_sddl`] keeps that entry once the launch
/// arrives.
pub fn runner_token_first_sddl(desktop_user: &Sid) -> String {
    format!("{RUNNER_TOKEN_FIRST_SDDL}(A;;0x{RUNNER_TOKEN_QUERY:x};;;{desktop_user})")
}

/// The end sweeper's threads' DACL (`runner/sweeper.rs`): `SYSTEM` full and `OWNER RIGHTS` held
/// to `READ_CONTROL`, nobody else. Unlike the inner's ([`runner_thread_sddl`]) it names no
/// desktop user: the sweeper takes nothing but `--sweep`, so it doesn't know that user, and the
/// outer needs nothing of its threads (it waits on the sweeper's process handle, which it holds
/// from the logon).
pub const SWEEPER_THREAD_SDDL: &str = "D:P(A;;GA;;;SY)(A;;RC;;;OW)";

/// The end sweeper's default DACL once shut: [`SWEEPER_THREAD_SDDL`]'s entries, so a thread the
/// system starts in it later (COM's and RPC's, for the scheduled tasks) is shut
/// too, and the pool user, its owner, keeps no implied `WRITE_DAC`.
pub const SWEEPER_DEFAULT_DACL: &str = "D:(A;;GA;;;SY)(A;;RC;;;OW)";

/// What the end sweeper's own user may do to its primary token for the COM step, once its sweep
/// left no process of that user: `TOKEN_QUERY` (`0x8`), `TOKEN_DUPLICATE` (`0x2`) and
/// `TOKEN_IMPERSONATE` (`0x4`), the rights the calls that read, copy and take on a token document
/// (`GetTokenInformation`: `TOKEN_QUERY`; `DuplicateTokenEx`: `TOKEN_DUPLICATE`;
/// `ImpersonateLoggedOnUser`: `TOKEN_QUERY` with `TOKEN_DUPLICATE` on a primary token,
/// `TOKEN_IMPERSONATE` on an impersonation one), which COM and RPC make on the caller's own token
/// to give a call its identity. With the token shut to `SYSTEM`, Task Scheduler and BITS (which
/// the sweeper no longer calls) both failed with `E_OUTOFMEMORY` (14), as the inner's did when
/// it still made COM calls after its own token's shut, before its threads' and default DACL's.
/// No `TOKEN_ADJUST_*`, `TOKEN_ASSIGN_PRIMARY`, `WRITE_DAC` or `WRITE_OWNER`.
pub const SWEEPER_COM_TOKEN_RIGHTS: u32 = 0x0000_000e;

/// The end sweeper's primary token's DACL for the COM step: [`RUNNER_TOKEN_FIRST_SDDL`]'s
/// entries and the pool user's [`SWEEPER_COM_TOKEN_RIGHTS`] (`runner/sweeper_flow.rs`).
pub fn sweeper_com_token_sddl(pool_user: &Sid) -> String {
    format!("D:P(A;;GA;;;SY)(A;;0x{SWEEPER_COM_TOKEN_RIGHTS:x};;;{pool_user})(A;;RC;;;OW)")
}

/// `SYSTEM`'s rights on the agent's window station: `WINSTA_ALL_ACCESS` (`0x37F`) and the
/// standard rights (`0xF0000`).
pub const STATION_SYSTEM_RIGHTS: u32 = 0x000F_037F;
/// `SYSTEM`'s rights on the agent's desktop: `DESKTOP`'s every right (`0x1FF`) and the
/// standard rights.
pub const DESKTOP_SYSTEM_RIGHTS: u32 = 0x000F_01FF;
/// The pool user's rights on the agent's window station: `READ_CONTROL` (`0x20000`) and the
/// station's use rights `0x7F` (enumerate and make desktops, read and write attributes, the
/// clipboard, global atoms, exit Windows). Less makes user32 fail to start the agent's GUI DLLs
/// (`0x45A`, `ERROR_DLL_INIT_FAILED`); no `WRITE_DAC`, `WRITE_OWNER` or `DELETE`.
pub const STATION_POOL_RIGHTS: u32 = 0x0002_007F;
/// The pool user's rights on the agent's desktop: read objects, make windows and menus,
/// enumerate, write objects (`0xC7`); no hooks, no journal, no switch. The creation descriptor
/// does not grant these to the pool user: it grants them to the desktop user, through
/// [`DESKTOP_MAKE_RIGHTS`].
pub const DESKTOP_POOL_RIGHTS: u32 = 0x0000_00C7;
/// The pool user's rights on the agent's desktop once it is final: [`DESKTOP_POOL_RIGHTS`] and
/// `READ_CONTROL` (`0x200C7`), so Chrome's own sandbox can read the desktop's DACL when it builds
/// child desktops. No `WRITE_DAC`, `WRITE_OWNER` or `DELETE`.
pub const DESKTOP_POOL_FINAL_RIGHTS: u32 = 0x0002_0000 | DESKTOP_POOL_RIGHTS;
/// The desktop user's rights on both once set up: `READ_CONTROL | WRITE_DAC | WRITE_OWNER`
/// (`0xE0000`), for the owner's own descriptor work and nothing of their use.
pub const CREATOR_RIGHTS: u32 = 0x000E_0000;
/// `OWNER RIGHTS`' rights in both creation descriptors: `READ_CONTROL` alone. The pool user
/// owns the station until the desktop user takes it over, and this entry takes away the owner's
/// implied `WRITE_DAC` and grants it no use right: no process of the pool user's rewrites the
/// DACL, or makes `Default` before the desktop user does, in that window.
pub const CREATION_OWNER_RIGHTS: u32 = 0x0002_0000;
/// `WINSTA_CREATEDESKTOP`.
const WINSTA_CREATEDESKTOP: u32 = 0x8;
/// What the desktop user asks for when it opens the station the pool user's logon made, and what
/// the station's creation DACL grants it: [`CREATOR_RIGHTS`] and `WINSTA_CREATEDESKTOP`, to make
/// `Default` on it.
pub const STATION_OPEN_RIGHTS: u32 = CREATOR_RIGHTS | WINSTA_CREATEDESKTOP;
/// What the desktop user asks for when it makes the desktop, and what the desktop's creation
/// DACL grants it: [`CREATOR_RIGHTS`] and [`DESKTOP_POOL_RIGHTS`] (`CreateDesktopW` asked for
/// `READ_CONTROL | WRITE_DAC | WRITE_OWNER` alone fails with 8).
pub const DESKTOP_MAKE_RIGHTS: u32 = CREATOR_RIGHTS | DESKTOP_POOL_RIGHTS;
/// What the pool user's logon asks for when it makes the station: the
/// [`CREATION_OWNER_RIGHTS`] its creation DACL grants `OWNER RIGHTS`, the pool user being its
/// owner then.
pub const STATION_MAKE_RIGHTS: u32 = CREATION_OWNER_RIGHTS;

/// The agent's window station's descriptor as the pool user's logon makes it
/// (`runner/station.rs`): `D:P(A;;0xF037F;;;SY)(A;;0xE0008;;;<desktop user>)(A;;0x20000;;;OW)`.
/// The desktop user gets [`STATION_OPEN_RIGHTS`], the owner (the pool user then)
/// [`CREATION_OWNER_RIGHTS`]. Never the default DACL, which grants `Everyone` and `RESTRICTED`
/// `0xF037F`.
pub fn station_creation_sddl(creator: &Sid) -> String {
    format!(
        "D:P(A;;0x{STATION_SYSTEM_RIGHTS:X};;;SY)(A;;0x{STATION_OPEN_RIGHTS:X};;;{creator})\
         (A;;0x{CREATION_OWNER_RIGHTS:X};;;OW)"
    )
}

/// The agent's desktop's descriptor as the desktop user makes it:
/// `D:P(A;;0xF01FF;;;SY)(A;;0xE00C7;;;<desktop user>)(A;;0x20000;;;OW)`.
pub fn desktop_creation_sddl(creator: &Sid) -> String {
    format!(
        "D:P(A;;0x{DESKTOP_SYSTEM_RIGHTS:X};;;SY)(A;;0x{DESKTOP_MAKE_RIGHTS:X};;;{creator})\
         (A;;0x{CREATION_OWNER_RIGHTS:X};;;OW)"
    )
}

/// The agent's window station's last descriptor, set by the desktop user once it owns it:
/// `D:P(A;;0xF037F;;;SY)(A;;0xE0000;;;<desktop user>)(A;;0x2007F;;;<pool user>)S:(ML;;NW;;;LW)`,
/// the matter SID given the pool user's rights after the pool user's when the launch restricts
/// with one. Protected, labelled Low: without the label a Low agent hangs in conhost's start
/// (PowerShell and `whoami` hang). No `OWNER RIGHTS` entry: the owner is the desktop user by
/// then, and one would give the lawyer's processes, Low ones included, the pool's use rights; the
/// owner keeps its implied `READ_CONTROL | WRITE_DAC` and the explicit [`CREATOR_RIGHTS`], enough
/// to clean up. No logon SID, so neither the lawyer's processes nor another runner's agent opens
/// it.
pub fn station_sddl(creator: &Sid, pool_user: &Sid, matter: Option<&Sid>) -> String {
    final_sddl(
        STATION_SYSTEM_RIGHTS,
        STATION_POOL_RIGHTS,
        creator,
        pool_user,
        matter,
    )
}

/// The agent's desktop's last descriptor: `D:P(A;;0xF01FF;;;SY)(A;;0xE0000;;;<desktop user>)
/// (A;;0x200C7;;;<pool user>)S:(ML;;NW;;;LW)`, the matter SID and the owner as in [`station_sddl`].
pub fn desktop_sddl(creator: &Sid, pool_user: &Sid, matter: Option<&Sid>) -> String {
    final_sddl(
        DESKTOP_SYSTEM_RIGHTS,
        DESKTOP_POOL_FINAL_RIGHTS,
        creator,
        pool_user,
        matter,
    )
}

/// [`station_sddl`]'s and [`desktop_sddl`]'s shape with their masks.
fn final_sddl(
    system: u32,
    pool: u32,
    creator: &Sid,
    pool_user: &Sid,
    matter: Option<&Sid>,
) -> String {
    let matter = matter
        .map(|sid| format!("(A;;0x{pool:X};;;{sid})"))
        .unwrap_or_default();
    format!(
        "D:P(A;;0x{system:X};;;SY)(A;;0x{CREATOR_RIGHTS:X};;;{creator})\
         (A;;0x{pool:X};;;{pool_user}){matter}S:(ML;;NW;;;LW)"
    )
}

/// Whether `sid` is a logon SID: `S-1-5-5-X-Y`.
pub fn is_logon_sid(sid: &Sid) -> bool {
    let parts: Vec<&str> = sid.as_str().split('-').collect();
    parts.len() == 6 && parts.get(..4) == Some(&["S", "1", "5", "5"][..])
}

/// Refuses a SID that can't be a pool user or a matter: anything outside `S-1-5-21-…` (a machine
/// or domain account), which leaves out every well-known group and service SID. The desktop
/// user is held to [`Sid::is_user_account`] instead, which takes an Entra ID user too.
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
#[cfg(test)]
mod window_tests;

// Two attributes, not `all(windows, test)`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
