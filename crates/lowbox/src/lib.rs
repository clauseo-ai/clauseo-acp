//! Launches a program in an `AppContainer` (`LowBox`) token on Windows.
//!
//! The first user is the Windows browser: chrome-headless-shell runs as a `LowBox` child of the
//! browser account `B_s`, with a package SID, no capabilities and a namespace the owner builds.
//! The work splits in two, as the processes do:
//! - **The owner** (the lawyer's outer process, in the desktop session) builds the package's
//!   object namespace with [`Namespace::build`] and hands its handles to the guard, which runs as
//!   `B_s`, through `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` or `DuplicateHandle`, with
//!   [`NamespaceHandles::to_arg`] on the guard's command line. It applies the grants that
//!   [`grants`] names.
//! - **The guard** (`B_s`) takes them back with [`NamespaceHandles::from_inherited`], which checks
//!   each value before use, and starts the program with [`LowBoxCommand::spawn_suspended`]:
//!   `NtCreateLowBoxToken` over its own token, then `CreateProcessAsUserW` on its own station and
//!   desktop. It puts the child in its Job Object before [`Spawned::resume`].
//!
//! `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` and `CreateAppContainerToken` fail with error 5
//! for a second user, so neither is used.
//!
//! The package SID, the argument quoting, the handle list's parse and checks, and the grants are
//! plain Rust on every OS; the calls into Windows are in the `windows` module.
//!
//! **Residual: inheritable handles and other starts.** While a launch runs, the namespace's
//! handles are inheritable ([`NamespaceHandles::inheritable`]), under
//! `clauseo_process::spawn_lock()`. That lock is taken by `spawn_in_tree` and lowbox's own
//! starts. `mcp/src/browser_guard/job/spawn.rs` must take the same lock around its inheritable
//! handles when mcp adopts lowbox (a follow-up). Process starts by third-party code inside the
//! host are a residual: one that starts a child without a handle list in that window could pass
//! the handles on.

#[cfg_attr(
    all(not(windows), not(test)),
    expect(
        dead_code,
        reason = "the windows module's environment block; tested on every OS"
    )
)]
mod envblock;
mod error;
mod grants;
#[cfg_attr(
    all(not(windows), not(test)),
    expect(
        dead_code,
        reason = "the checks the windows module runs; tested on every OS"
    )
)]
mod handles;
#[cfg_attr(
    all(not(windows), not(test)),
    expect(
        dead_code,
        reason = "the windows module's names and descriptors; tested on every OS"
    )
)]
mod layout;
mod package;
mod quote;
// The token rules: plain Rust, tested on every OS, used by the windows module.
#[cfg(any(windows, test))]
mod token_check;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use self::windows::{
    Inheritable, LowBoxCommand, Namespace, NamespaceHandles, Spawned, Stdio, UNRESUMED_EXIT,
};
pub use error::{Error, HandleProblem};
pub use grants::{BrowserFolders, Grant, Rights, grants};
pub use handles::{HANDLE_COUNT, ObjectKind, Slot, parse_handle_arg};
pub use package::{PackageName, Sid, package_sid};
pub use quote::command_line;
