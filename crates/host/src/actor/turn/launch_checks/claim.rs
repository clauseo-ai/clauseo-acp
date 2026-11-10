//! The launch-prep thread's claims for a session ([`claim`]): its browser, egress proxy and
//! case-law relay, and the `PATH` its shells get, which needs the browser's answer.

use std::path::Path;

use super::{Claims, RunFolders, straggler};

/// The session's egress proxy, browser claims and case-law relay (`claims`), made on the
/// launch-prep thread. `slot`: whether a browser slot is there after the prep's slot wait.
pub(super) fn claim(run: &mut RunFolders, data: &Path, claims: Claims, slot: bool) {
    let Claims {
        browser,
        case_law,
        egress,
        held,
        path,
    } = claims;
    let browser = if slot || browser.shell() {
        browser
    } else {
        browser.skipped()
    };
    run.browser_shell = browser.shell();
    let prepared = browser.prepare(data, &run.key.id());
    (run.chrome, run.browser_session) = (prepared.chrome, prepared.session);
    (run.browser_blocked, run.browser_policy_read) = (prepared.policy_block, prepared.policy_read);
    if run.browser_shell && run.chrome.is_some() {
        super::browser::shell_folders(run, data);
    } else if run.chrome.is_some() && run.browser_session.is_some() {
        super::browser::entry_folder(run, data);
    }
    // The launch offers the browser only once its Chrome was found and claimed.
    let offered = run.chrome.is_some() && run.browser_session.is_some();
    run.egress = egress.open(run.key, offered);
    run.case_law = case_law.open(run);
    let parts = straggler::Parts {
        browser: run.browser_session.clone(),
        case_law: run.case_law.clone(),
        egress: run.egress.clone(),
    };
    run.lease.keep(held, parts);
    // Last: the tools folder goes on `PATH` only once the browser is known.
    run.path = path.resolve(run);
}
