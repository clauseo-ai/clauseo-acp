//! The browser's folders in Claude's sandbox (`agent/loads/sandbox.rs`, `browser_denied`).

use std::path::PathBuf;

use serde_json::json;

use super::super::sandbox::{ClaudeSandbox, SandboxOs};
use super::{Session, mac, texts};

/// The browser's folders (`<data>/browser`, the session's guard folder with Chrome's profile in
/// it, and `<data>/browsers`) are denied for reads and writes on macOS and Linux, in each
/// spelling of `<data>`, and nothing allowed opens them.
#[test]
#[cfg_attr(windows, ignore = "Claude's sandbox is macOS and Linux only")]
fn the_browsers_folders_are_denied_on_mac_and_linux() {
    let mut session = Session::new("s-1");
    session.data = vec![PathBuf::from("/srv/real"), PathBuf::from("/srv/link")];
    session.browser = PathBuf::from("/srv/link/browser/run/s-1");
    for os in [SandboxOs::Mac, SandboxOs::Linux] {
        let sandbox = ClaudeSandbox::for_os(os, &session.paths()).unwrap();
        for data in ["/srv/real", "/srv/link"] {
            for denied in [
                format!("{data}/browser"),
                format!("{data}/browsers"),
                format!("{data}/browser/run/s-1"),
            ] {
                assert!(sandbox.deny_read.contains(&denied), "{os:?} read {denied}");
                assert!(
                    sandbox.deny_write.contains(&denied),
                    "{os:?} write {denied}"
                );
                for open in sandbox.allow_read.iter().chain(&sandbox.allow_write) {
                    let under = denied.strip_prefix(open.as_str());
                    assert!(
                        !under.is_some_and(|rest| rest.is_empty() || rest.starts_with('/')),
                        "{os:?}: {open} opens {denied}"
                    );
                }
            }
        }
        let filesystem = &sandbox.settings()["filesystem"];
        let listed = |key: &str| texts(&filesystem[key]).contains(&"/srv/real/browsers");
        assert!(listed("denyRead") && listed("denyWrite"), "{os:?}");
        assert_eq!(
            sandbox.settings()["enableWeakerNestedSandbox"],
            json!(false)
        );
    }
}

/// A data folder with `[` in its name: the browser's denies are escaped (`[[]`) in every list on
/// macOS, so Claude Code's glob names the folder itself, and stay as written in Linux's write
/// list, which takes no glob.
#[test]
#[cfg_attr(windows, ignore = "Claude's sandbox is macOS and Linux only")]
fn a_bracket_in_the_data_folder_is_escaped_in_the_browsers_denies() {
    let mut session = Session::new("s-1");
    session.data = vec![PathBuf::from("/srv/Clauseo [1]")];
    session.browser = PathBuf::from("/srv/Clauseo [1]/browser/run/s-1");
    let escaped = "/srv/Clauseo [[]1]";
    let mac = mac(&session.paths()).unwrap();
    let filesystem = &mac.settings()["filesystem"];
    for key in ["denyRead", "denyWrite"] {
        let list = texts(&filesystem[key]);
        for name in ["browser", "browsers", "browser/run/s-1"] {
            assert!(
                list.contains(&format!("{escaped}/{name}").as_str()),
                "{key} {name}"
            );
        }
        assert!(
            !list.iter().any(|p| p.contains("[1]") && !p.contains("[[]")),
            "{key} {list:?}"
        );
    }
    let linux = ClaudeSandbox::for_os(SandboxOs::Linux, &session.paths()).unwrap();
    assert!(linux.deny_read.contains(&format!("{escaped}/browsers")));
    assert!(
        linux
            .deny_write
            .contains(&"/srv/Clauseo [1]/browsers".to_owned())
    );
}
