//! The headless-shell proof row (CI: `arm64-browser-proof.yml`, elevated, on windows-11-arm with
//! the x64 shell under Prism and on x64): chrome-headless-shell runs as a `LowBox` child of a
//! test account, the guard's role, exactly as the crate's two halves would run it, and the CI
//! log alone shows the ruling.
//!
//! The owner (this test, elevated) makes the test account, its folders and the package's grants,
//! the recipe's machine-wide ACEs and loopback exemption, a private window station, a local
//! proxy and the namespace (`Namespace::build`), then starts the helper (`lowbox_helper`, this
//! binary again) as the account with `CreateProcessWithLogonW` on the private station, copies
//! the namespace handles into it (`NamespaceHandles::duplicate_into`) and tells it where
//! everything is. The helper takes the handles (`from_inherited`), starts the shell
//! (`LowBoxCommand::spawn_suspended`) in its Job Object and reports what it can open and see
//! of chrome's processes while the proxy holds the page. The owner checks the report, the
//! screenshot and the proxy, then undoes everything, each step checked.
//!
//! Chrome speaks CDP over two anonymous pipes (`--remote-debugging-pipe`), never a TCP port: no
//! `--remote-debugging-port`, though the CI brief listed it, since it opens a listener inside the
//! `AppContainer`. The helper drives the page and the screenshot over CDP (`proof/cdp.rs`) and
//! the row asserts no chrome process listens on a TCP port (`proof/listen.rs`).
//!
//! Inputs: `CLAUSEO_ELEVATED_TESTS=1`, `CLAUSEO_TEST_CHROME_SHELL` (the shell's full path) and
//! `CLAUSEO_TEST_ARTIFACTS` (a folder CI uploads); a missing one fails the row.
//!
//! A test crate root resolves modules beside itself, so each submodule names its path. Each
//! carries `#[cfg(test)]` so clippy treats its helpers as test code.
#![cfg(windows)]

#[cfg(test)]
#[path = "proof/win.rs"]
mod win;

#[cfg(test)]
#[path = "proof/write.rs"]
mod write;

#[cfg(test)]
#[path = "proof/account.rs"]
mod account;

#[cfg(test)]
#[path = "proof/acl.rs"]
mod acl;

#[cfg(test)]
#[path = "proof/loopback.rs"]
mod loopback;

#[cfg(test)]
#[path = "proof/station.rs"]
mod station;

#[cfg(test)]
#[path = "proof/proxy.rs"]
mod proxy;

#[cfg(test)]
#[path = "proof/launch.rs"]
mod launch;

#[cfg(test)]
#[path = "proof/helper.rs"]
mod helper;

#[cfg(test)]
#[path = "proof/cdp.rs"]
mod cdp;

#[cfg(test)]
#[path = "proof/inspect.rs"]
mod inspect;

#[cfg(test)]
#[path = "proof/listen.rs"]
mod listen;

#[cfg(test)]
#[path = "proof/evidence.rs"]
mod evidence;

#[cfg(test)]
#[path = "proof/owner.rs"]
mod owner;

#[cfg(test)]
#[path = "proof/teardown.rs"]
mod teardown;

/// The row: set-up, run, assertions and cleanup (see the module's doc).
#[test]
#[ignore = "run by arm64-browser-proof.yml"]
fn lowbox_headless_shell_row() {
    owner::row();
}

/// The helper's half, run by the row as the test account; anywhere else (argv[0] is not the
/// row's mark) it returns at once.
#[test]
#[ignore = "re-invoked by lowbox_headless_shell_row"]
fn lowbox_helper() {
    if std::env::args().next().as_deref() == Some(launch::HELPER_ARG0) {
        helper::run();
    }
}
