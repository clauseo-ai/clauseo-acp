//! What the guard may look at on the disk. A path an agent names is never handed to the file
//! system when it is, or leads to, a network location: touching a share as the lawyer signs
//! them in to a server the agent picked. The path is walked name by name and each link's target
//! checked by its text before it is followed ([`crate::network_paths::resolve::walk`],
//! fail-closed); one it does not vouch for is judged by its text alone. So is a path that lies,
//! by its text, in a matter folder the lawyer keeps on a share: never touched.

use std::path::{Path, PathBuf};

use super::MAX_RESOLVED;
use crate::network_paths::resolve::Disk;
use crate::network_paths::{NetEnv, WalkedPath, touchable};

/// The network rules and the matter folder a guard run checks paths against.
pub(super) struct Reach {
    net: NetEnv,
    matter: PathBuf,
}

impl Reach {
    /// This system's rules (its remote mounts read now, never a path) for the matter `matter`.
    pub(super) fn new(matter: &Path) -> Self {
        Self {
            net: NetEnv::current(),
            matter: matter.to_path_buf(),
        }
    }

    /// Where `path` (absolute) leads, when the guard may stat, resolve or open it: the walked
    /// path, which the guard uses from then on and never resolves the agent's text again
    /// ([`touchable`]), and opens only with no link followed
    /// ([`crate::network_paths::open_no_follow`]). `None` for one it may not touch, and for one longer than
    /// [`MAX_RESOLVED`] bytes, which is not walked (its time grows with its length) and so is
    /// judged by its text alone.
    pub(super) fn walked(&self, path: &Path) -> Option<WalkedPath> {
        self.walked_on(path, &Disk::resolved(&self.net))
    }

    /// [`Self::walked`], walking on `disk`.
    fn walked_on(&self, path: &Path, disk: &Disk<'_>) -> Option<WalkedPath> {
        if path.as_os_str().len() > MAX_RESOLVED {
            return None;
        }
        touchable(path, &self.matter, disk)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    #[cfg(unix)]
    use std::path::PathBuf;

    use super::Reach;
    use crate::network_paths::NetEnv;
    use crate::network_paths::resolve::Disk;
    #[cfg(unix)]
    use crate::network_paths::resolve::tests::Fake;
    use crate::network_paths::resolve::tests::FakeFs;
    use crate::network_paths::tests::plain_net;

    fn reach(net: NetEnv, matter: &str) -> Reach {
        Reach {
            net,
            matter: matter.into(),
        }
    }

    fn may(reach: &Reach, path: &str, fs: &FakeFs) -> bool {
        let real = |_: &Path| None;
        let disk = Disk {
            fs,
            real: &real,
            net: &reach.net,
        };
        reach.walked_on(Path::new(path), &disk).is_some()
    }

    /// A network path, or a local link to one, is never looked at past the link; nor is a path
    /// in a matter kept on a share, nor another server's; a local path is walked as before.
    #[cfg(unix)]
    #[test]
    fn the_guard_never_looks_at_a_network_path_an_agent_names() {
        let local = reach(plain_net(false), "/home/a/matter");
        let fs = FakeFs::with(&[("/home/a/matter/x", Fake::Link(r"\\evil\s".into()))]);
        for path in [r"\\evil\s\x.docx", r"\\?\UNC\evil\s", "/net/evil/s"] {
            assert!(!may(&local, path, &fs), "{path}");
        }
        assert!(fs.touched().is_empty(), "{:?}", fs.touched());
        assert!(!may(&local, "/home/a/matter/x/doc.docx", &fs));
        let past: Vec<PathBuf> = fs
            .touched()
            .into_iter()
            .filter(|p| p.starts_with("/home/a/matter/x") && p != Path::new("/home/a/matter/x"))
            .collect();
        assert!(past.is_empty(), "{past:?}");
        assert!(may(&local, "/home/a/matter/brief.docx", &fs));
    }

    /// In a matter kept on a share nothing is looked at: not a path in it, not a relative one,
    /// not another server's (on every system: Windows' forms, no local path).
    #[test]
    fn the_guard_never_looks_at_a_path_in_a_share_matter() {
        let share = reach(plain_net(true), r"\\srv\share\matter");
        let none = FakeFs::default();
        for path in [
            r"\\srv\share\matter\brief.docx",
            "brief.docx",
            r"\\evil\share\matter\brief.docx",
        ] {
            assert!(!may(&share, path, &none), "{path}");
        }
        assert!(none.touched().is_empty(), "{:?}", none.touched());
    }

    /// The guard gets the walked path back, links followed and `..` taken, and uses it from
    /// then on: the agent's text is never resolved again.
    #[cfg(unix)]
    #[test]
    fn the_guard_uses_the_walked_path() {
        let local = reach(plain_net(false), "/home/a/matter");
        let fs = FakeFs::with(&[
            ("/home/a/matter/Deals/spa.docx", Fake::File),
            ("/home/a/matter/l", Fake::Link("Deals".into())),
        ]);
        let real = |_: &Path| None;
        let disk = Disk {
            fs: &fs,
            real: &real,
            net: &local.net,
        };
        assert_eq!(
            local
                .walked_on(Path::new("/home/a/matter/l/../l/spa.docx"), &disk)
                .map(|walked| walked.as_path().to_path_buf()),
            Some(PathBuf::from("/home/a/matter/Deals/spa.docx"))
        );
        assert_eq!(local.walked_on(Path::new(r"\\evil\s\x"), &disk), None);
    }
}
