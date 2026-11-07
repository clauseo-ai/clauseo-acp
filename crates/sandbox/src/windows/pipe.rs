//! The pipes the runner makes (S15 `PR5`, D11): four named pipes per launch, made by the outer
//! runner (the desktop user) before it starts the inner runner (the pool user), whose agent gets
//! three of them as its stdio.
//!
//! - **Names:** `\\.\pipe\clauseo-run-<32 hex>-{ctl,in,out,err}`, the hex 128 random bits per
//!   launch ([`PipeSet::random`]). Names can be listed by anyone, so the name is no secret: what
//!   protects each pipe is its DACL, one instance, and the outer's check of who connected.
//! - **DACL** ([`pipe_sddl`]), set explicitly on every pipe: the desktop user and the pool user,
//!   protected, nothing else. `Everyone` stays in the (B) token (D11), and NPFS gives a pipe made
//!   with no descriptor `Everyone` read; with this DACL another matter's agent (another pool
//!   user) opening any of the four gets error 5. No label: a pipe without one is Medium with
//!   no-write-up, so even this matter's own Low agent can't open a write end by name; it holds
//!   only the handles the inner runner opened and passed down.
//! - **One instance each**, made with `FILE_FLAG_FIRST_PIPE_INSTANCE` (so a squatter that made
//!   the name first fails the launch rather than receive it) and `PIPE_REJECT_REMOTE_CLIENTS`.
//!   The outer checks every client is the inner runner's pid, and on `ctl` that the pid's token
//!   user is the pool user, before it sends or relays a byte (`runner::outer`).
//! - `ctl` carries the launch to the inner runner and its report back, as frames
//!   (`runner::frame`); `in`, `out` and `err` are the agent's stdin, stdout and stderr.
//!
//! The names and the DACL are built with no Windows call, so their tests run on every OS.

use std::fmt;

use super::sids::Sid;

#[cfg(windows)]
pub(in crate::windows) mod os;

/// The start of every runner pipe's name, after `\\.\pipe\`.
pub const PIPE_PREFIX: &str = "clauseo-run-";
/// Where named pipes live.
pub const PIPE_ROOT: &str = r"\\.\pipe\";

/// One of a launch's four pipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The launch and the inner runner's report, both ways.
    Ctl,
    /// The agent's stdin: the outer writes, the agent reads.
    In,
    /// The agent's stdout: the agent writes, the outer reads.
    Out,
    /// The agent's stderr: the agent writes, the outer reads.
    Err,
}

impl Role {
    /// The four, in the order the outer makes them and the inner opens them.
    pub const ALL: [Role; 4] = [Role::Ctl, Role::In, Role::Out, Role::Err];

    /// The name's last part.
    pub fn suffix(self) -> &'static str {
        match self {
            Role::Ctl => "ctl",
            Role::In => "in",
            Role::Out => "out",
            Role::Err => "err",
        }
    }
}

/// Why a pipe set's name was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a runner pipe set: {0:?}")]
pub struct NotAPipeSet(pub String);

/// A launch's four pipe names, from one base `clauseo-run-<32 lowercase hex>`.
#[derive(Clone, PartialEq, Eq)]
pub struct PipeSet {
    base: String,
}

impl PipeSet {
    /// A new set with 128 random bits from the OS.
    pub fn random() -> std::io::Result<PipeSet> {
        let mut bits = [0u8; 16];
        getrandom::fill(&mut bits).map_err(|error| std::io::Error::other(error.to_string()))?;
        let hex = hex(&bits);
        Ok(PipeSet {
            base: format!("{PIPE_PREFIX}{hex}"),
        })
    }

    /// The set whose base is `base`, exactly as [`PipeSet::base`] gives it.
    pub fn parse(base: &str) -> Result<PipeSet, NotAPipeSet> {
        let hex = base
            .strip_prefix(PIPE_PREFIX)
            .ok_or_else(|| NotAPipeSet(base.to_owned()))?;
        if hex.len() != 32
            || !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(NotAPipeSet(base.to_owned()));
        }
        Ok(PipeSet {
            base: base.to_owned(),
        })
    }

    /// The base, `clauseo-run-<32 hex>`: what the inner runner gets on its command line.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// The full name of `role`'s pipe: `\\.\pipe\<base>-<suffix>`.
    pub fn name(&self, role: Role) -> String {
        format!("{PIPE_ROOT}{}-{}", self.base, role.suffix())
    }
}

impl fmt::Debug for PipeSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PipeSet").field(&self.base).finish()
    }
}

/// Every runner pipe's descriptor: protected, the desktop user and the pool user with full
/// access, and no one else (D11).
pub fn pipe_sddl(desktop_user: &Sid, pool_user: &Sid) -> String {
    format!("D:P(A;;GA;;;{desktop_user})(A;;GA;;;{pool_user})")
}

/// `bytes` as lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        text.push(char::from_digit(u32::from(byte & 0xf), 16).unwrap_or('0'));
    }
    text
}

#[cfg(test)]
mod tests;
