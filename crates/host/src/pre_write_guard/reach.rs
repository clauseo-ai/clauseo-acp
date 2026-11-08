//! What the guard may look at on the disk. A path an agent names is never handed to the file
//! system when it is, or leads to, a network location: touching a share as the lawyer signs
//! them in to a server the agent picked. The path is walked name by name and each link's target
//! checked by its text before it is followed ([`crate::network_paths::resolve`]); one that
//! stops at a network location is judged by its text alone. A path that lies, by its text, in a
//! matter folder the lawyer keeps on a share is the lawyer's own and is looked at as before.

use std::path::{Path, PathBuf};

use super::MAX_RESOLVED;
use crate::network_paths::resolve::{Fs, RealFs};
use crate::network_paths::{NetEnv, may_touch};

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

    /// Whether the guard may stat, resolve or open `path` (absolute). Never one longer than
    /// [`MAX_RESOLVED`] bytes, which is not walked (its time grows with its length) and so is
    /// judged by its text alone.
    pub(super) fn may_touch(&self, path: &Path) -> bool {
        self.may_touch_on(path, &RealFs)
    }

    /// [`Self::may_touch`], walking on `fs`.
    fn may_touch_on(&self, path: &Path, fs: &dyn Fs) -> bool {
        if path.as_os_str().len() > MAX_RESOLVED {
            return false;
        }
        may_touch(path, &self.matter, &self.net, fs)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Reach;
    use crate::network_paths::NetEnv;
    use crate::network_paths::resolve::tests::{Fake, FakeFs};
    use crate::network_paths::tests::plain_net;

    fn reach(net: NetEnv, matter: &str) -> Reach {
        Reach {
            net,
            matter: matter.into(),
        }
    }

    /// A network path, or a local link to one, is never looked at past the link; a path in a
    /// matter kept on a share is the lawyer's own; a local path is walked as before.
    #[test]
    fn the_guard_never_looks_at_a_network_path_an_agent_names() {
        let local = reach(plain_net(false), "/home/a/matter");
        let fs = FakeFs::with(&[("/home/a/matter/x", Fake::Link(r"\\evil\s".into()))]);
        for path in [r"\\evil\s\x.docx", r"\\?\UNC\evil\s", "/net/evil/s"] {
            assert!(!local.may_touch_on(Path::new(path), &fs), "{path}");
        }
        assert!(fs.touched().is_empty(), "{:?}", fs.touched());
        assert!(!local.may_touch_on(Path::new("/home/a/matter/x/doc.docx"), &fs));
        let past: Vec<_> = fs
            .touched()
            .into_iter()
            .filter(|p| p.starts_with("/home/a/matter/x") && p != Path::new("/home/a/matter/x"))
            .collect();
        assert!(past.is_empty(), "{past:?}");
        assert!(local.may_touch_on(Path::new("/home/a/matter/brief.docx"), &fs));
        let windows = plain_net(true);
        let share = reach(windows, r"\\srv\share\matter");
        let none = FakeFs::default();
        assert!(share.may_touch_on(Path::new(r"\\srv\share\matter\brief.docx"), &none));
        assert!(!share.may_touch_on(Path::new(r"\\evil\share\matter\brief.docx"), &none));
        assert!(none.touched().is_empty(), "{:?}", none.touched());
    }
}
