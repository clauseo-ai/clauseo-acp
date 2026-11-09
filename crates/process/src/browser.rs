//! Which processes of a tree are a web browser and its own children, for a tree whose stop
//! spares them (`LaunchSpec::spare_browsers`): Codex's login opens the lawyer's default browser
//! as its own child on Windows (`webbrowser`'s `AssocQueryStringW`, then a plain spawn), and a
//! browser started there is the lawyer's whole browser, every tab in it. The OS half that lists
//! the tree and ends the rest is `os/windows/spare.rs`; this part reads nothing from the OS, so
//! its tests run everywhere.

use std::path::Path;

use crate::descendants::Proc;

/// The image names taken as a browser whatever the default is: Edge, Chrome, Firefox, Brave,
/// Opera, Vivaldi and Internet Explorer.
pub(crate) const BROWSER_IMAGES: [&str; 7] = [
    "msedge.exe",
    "chrome.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "iexplore.exe",
];

/// The last part of `path` split at `/` or `\`, as Windows splits it on every OS.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Whether `exe` is a browser: the default http handler's executable (`default`), the whole
/// path compared ignoring ASCII case as Windows does, or a file named as one of
/// [`BROWSER_IMAGES`].
pub(crate) fn is_browser(exe: &Path, default: Option<&Path>) -> bool {
    let exe = exe.to_string_lossy();
    if default.is_some_and(|default| default.to_string_lossy().eq_ignore_ascii_case(&exe)) {
        return true;
    }
    let name = file_name(&exe);
    BROWSER_IMAGES
        .iter()
        .any(|image| image.eq_ignore_ascii_case(name))
}

/// The pids of `listed` to spare: each process `browser` says is one, and each process whose
/// parents, as listed, lead to one. A parent counts only if it started no later than its
/// child: a parent pid can name a process that exited, its pid since reused by a browser.
pub(crate) fn spared(listed: &[Proc], browser: impl Fn(&Proc) -> bool) -> Vec<u32> {
    let mut spared: Vec<&Proc> = listed.iter().filter(|proc| browser(proc)).collect();
    // Each pass adds the children of what is spared; a chain is at most `listed` long.
    loop {
        let more: Vec<&Proc> = listed
            .iter()
            .filter(|proc| !spared.iter().any(|kept| kept.pid == proc.pid))
            .filter(|proc| {
                spared
                    .iter()
                    .any(|kept| proc.parent == Some(kept.pid) && kept.start <= proc.start)
            })
            .collect();
        if more.is_empty() {
            break;
        }
        spared.extend(more);
    }
    spared.into_iter().map(|proc| proc.pid).collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{is_browser, spared};
    use crate::descendants::Proc;

    fn proc(pid: u32, parent: u32, start: u64, exe: &str) -> Proc {
        Proc {
            pid,
            parent: Some(parent),
            start,
            leads_group: false,
            exe: Some(PathBuf::from(exe)),
        }
    }

    #[test]
    fn a_browser_is_the_default_handler_or_a_known_image() {
        let default = Path::new(r"C:\Program Files\Arc\arc.exe");
        assert!(is_browser(
            Path::new(r"c:\program files\ARC\Arc.exe"),
            Some(default)
        ));
        assert!(is_browser(
            Path::new(r"C:\Program Files (x86)\Microsoft\Edge\Application\MSEDGE.EXE"),
            None
        ));
        assert!(is_browser(Path::new(r"C:\x\firefox.exe"), Some(default)));
        assert!(is_browser(Path::new("/opt/brave/brave.exe"), None));
        assert!(!is_browser(Path::new(r"C:\x\codex.exe"), Some(default)));
        assert!(!is_browser(Path::new(r"C:\x\node.exe"), None));
        assert!(!is_browser(Path::new(r"C:\x\conhost.exe"), None));
        // A name that only ends like a browser's is not one.
        assert!(!is_browser(Path::new(r"C:\x\notchrome.exe"), None));
        assert!(!is_browser(Path::new(r"C:\x\arc.exe"), None));
    }

    /// The tree of `node codex.js login`: node, its conhost, codex.exe, and Edge with two
    /// children of its own. Edge and its children are spared; the rest are not.
    #[test]
    fn a_browser_and_its_children_are_spared_and_nothing_else() {
        let listed = [
            proc(20, 10, 2, r"C:\Windows\System32\conhost.exe"),
            proc(30, 10, 3, r"C:\app\codex.exe"),
            proc(40, 30, 4, r"C:\Edge\msedge.exe"),
            proc(50, 40, 5, r"C:\Edge\msedge.exe"),
            proc(60, 50, 6, r"C:\Edge\identity_helper.exe"),
            proc(70, 30, 7, r"C:\app\helper.exe"),
        ];
        let browser = |p: &Proc| is_browser(p.exe.as_deref().unwrap(), None);
        let mut kept = spared(&listed, browser);
        kept.sort_unstable();
        assert_eq!(kept, [40, 50, 60]);
    }

    /// A process whose parent pid was reused by a browser started after it is not the
    /// browser's child.
    #[test]
    fn a_reused_parent_pid_does_not_spare() {
        let listed = [
            proc(40, 30, 9, r"C:\Edge\msedge.exe"),
            proc(70, 40, 5, r"C:\app\helper.exe"),
        ];
        let browser = |p: &Proc| is_browser(p.exe.as_deref().unwrap(), None);
        assert_eq!(spared(&listed, browser), [40]);
    }

    #[test]
    fn a_tree_with_no_browser_spares_nothing() {
        let listed = [proc(30, 10, 3, r"C:\app\codex.exe")];
        let browser = |p: &Proc| is_browser(p.exe.as_deref().unwrap(), None);
        assert_eq!(spared(&listed, browser), Vec::<u32>::new());
    }
}
