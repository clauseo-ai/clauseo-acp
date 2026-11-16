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

/// What a default http handler that is no browser resolves to: a shell, Node, Codex itself, a
/// console host, or `rundll32` (a DLL's handler). Such a default is ignored and only
/// [`BROWSER_IMAGES`] count, so the login's own processes are never taken for a browser.
pub(crate) const NOT_BROWSERS: [&str; 7] = [
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "node.exe",
    "codex.exe",
    "conhost.exe",
    "rundll32.exe",
];

/// One listed process: its pid with its start, which together name it (a pid is reused).
pub(crate) type Key = (u32, u64);

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

/// Whether `default`, the http handler's executable, may count as a browser: none of
/// [`NOT_BROWSERS`].
pub(crate) fn usable_default(default: &Path) -> bool {
    let path = default.to_string_lossy();
    let name = file_name(&path);
    !NOT_BROWSERS
        .iter()
        .any(|not| not.eq_ignore_ascii_case(name))
}

/// `child`'s parent among `listed`: the process with its parent pid that started no later
/// than it (a parent pid can name a process that exited, its pid since reused).
fn parent_of<'a>(listed: &'a [Proc], child: &Proc) -> Option<&'a Proc> {
    listed
        .iter()
        .find(|proc| Some(proc.pid) == child.parent && proc.start <= child.start)
}

/// The processes of `listed` to spare: each process `browser` says is one, but never `root`
/// (the run's own process, which a default handler resolved to Node or Codex would name), and
/// every process whose parents, as listed, lead to one. So no spared process is an ancestor of
/// one left to end.
pub(crate) fn spared(listed: &[Proc], root: u32, browser: impl Fn(&Proc) -> bool) -> Vec<Key> {
    let mut spared: Vec<&Proc> = listed
        .iter()
        .filter(|proc| proc.pid != root && browser(proc))
        .collect();
    // Each pass adds the children of what is spared; a chain is at most `listed` long.
    loop {
        let more: Vec<&Proc> = listed
            .iter()
            .filter(|proc| !spared.iter().any(|kept| kept.pid == proc.pid))
            .filter(|proc| {
                parent_of(listed, proc)
                    .is_some_and(|parent| spared.iter().any(|kept| kept.pid == parent.pid))
            })
            .collect();
        if more.is_empty() {
            break;
        }
        spared.extend(more);
    }
    // Every child of a kept process is kept, so no kept process is an ancestor of one left to
    // end; the root, the ancestor of all, is never kept.
    spared
        .into_iter()
        .map(|proc| (proc.pid, proc.start))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{is_browser, spared, usable_default};
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

    fn named(p: &Proc) -> bool {
        is_browser(p.exe.as_deref().unwrap(), None)
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

    #[test]
    fn a_default_handler_that_is_no_browser_is_ignored() {
        assert!(usable_default(Path::new(r"C:\Program Files\Arc\arc.exe")));
        assert!(usable_default(Path::new(r"C:\Edge\msedge.exe")));
        for not in [
            r"C:\Windows\System32\cmd.exe",
            r"C:\Windows\System32\WindowsPowerShell\v1.0\POWERSHELL.EXE",
            r"C:\Program Files\PowerShell\7\pwsh.exe",
            r"C:\Program Files\nodejs\node.exe",
            r"C:\x\codex.exe",
            r"C:\Windows\System32\conhost.exe",
            r"C:\Windows\System32\rundll32.exe",
        ] {
            assert!(!usable_default(Path::new(not)), "{not}");
        }
    }

    /// The tree of `node codex.js login`: node (the root, 10), its conhost, codex.exe, and Edge
    /// with two children of its own. Edge and its children are spared; the rest are not.
    #[test]
    fn a_browser_and_its_children_are_spared_and_nothing_else() {
        let listed = [
            proc(10, 1, 1, r"C:\nodejs\node.exe"),
            proc(20, 10, 2, r"C:\Windows\System32\conhost.exe"),
            proc(30, 10, 3, r"C:\app\codex.exe"),
            proc(40, 30, 4, r"C:\Edge\msedge.exe"),
            proc(50, 40, 5, r"C:\Edge\msedge.exe"),
            proc(60, 50, 6, r"C:\Edge\identity_helper.exe"),
            proc(70, 30, 7, r"C:\app\helper.exe"),
        ];
        let mut kept = spared(&listed, 10, named);
        kept.sort_unstable();
        assert_eq!(kept, [(40, 4), (50, 5), (60, 6)]);
    }

    /// A process whose parent pid was reused by a browser started after it is not the
    /// browser's child.
    #[test]
    fn a_reused_parent_pid_does_not_spare() {
        let listed = [
            proc(40, 30, 9, r"C:\Edge\msedge.exe"),
            proc(70, 40, 5, r"C:\app\helper.exe"),
        ];
        assert_eq!(spared(&listed, 10, named), [(40, 9)]);
    }

    #[test]
    fn a_tree_with_no_browser_spares_nothing() {
        let listed = [proc(30, 10, 3, r"C:\app\codex.exe")];
        assert_eq!(spared(&listed, 10, named), []);
    }

    /// The run's own process is never spared, even named like a browser (a default handler
    /// that resolved to it).
    #[test]
    fn the_root_and_an_ancestor_of_the_rest_are_never_spared() {
        let listed = [
            proc(10, 1, 1, r"C:\Edge\msedge.exe"),
            proc(30, 10, 3, r"C:\app\codex.exe"),
        ];
        assert_eq!(spared(&listed, 10, named), []);
        // A browser's whole subtree goes with it, so nothing left to end sits below it.
        let listed = [
            proc(30, 10, 3, r"C:\app\codex.exe"),
            proc(40, 30, 4, r"C:\Edge\msedge.exe"),
            proc(50, 40, 6, r"C:\app\helper.exe"),
            proc(60, 50, 7, r"C:\app\helper.exe"),
        ];
        let mut kept = spared(&listed, 10, named);
        kept.sort_unstable();
        assert_eq!(kept, [(40, 4), (50, 6), (60, 7)]);
    }
}
