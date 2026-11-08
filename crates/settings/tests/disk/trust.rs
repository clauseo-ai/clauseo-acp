//! The trust check on a system `policy.json` ([`TrustedFilePolicy`]) on a real disk: a file an
//! administrator owns that no one else can change is read; a planted one (another owner, group
//! or other write, a symlink; on Windows a second name) is ignored: the effective settings are
//! those with no file, and the host log says once which source and why, with no path.
//!
//! A test cannot make a root-owned folder, so the Unix tests pass the test user's id as the
//! owner the check expects; production passes 0.

use clauseo_settings::{Effective, Locations, PolicySource, SystemFiles};
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::util::SubscriberInitExt as _;

/// A policy the check decides on: a value that locks when read.
const POLICY: &[u8] = br#"{"ui": {"theme": "dark"}}"#;

/// A warn-once flag of the test's own, so each source logs its first ignored read whatever
/// other tests in this process logged.
fn own_flag() -> &'static AtomicBool {
    Box::leak(Box::new(AtomicBool::new(false)))
}

/// What the host log got, in memory.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl io::Write for Log {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Log {
    type Writer = Log;

    fn make_writer(&'a self) -> Log {
        self.clone()
    }
}

/// The effective settings with `source` as the only policy source and no `settings.json` under
/// `root`, with what the log got while loading.
fn load(root: &Path, source: &dyn PolicySource) -> (Effective, String) {
    let log = Log::default();
    let guard = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .finish()
        .set_default();
    let effective = Effective::load(&Locations::under_root(root), &SystemFiles, &[source]);
    drop(guard);
    let text =
        String::from_utf8_lossy(&log.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned();
    (effective, text)
}

/// `source`'s file is ignored for `reason`: the effective settings are those with no policy, and
/// the log names the source `label` and the reason, never the path.
fn assert_ignored(root: &Path, source: &dyn PolicySource, label: &str, reason: &str) {
    let (effective, log) = load(root, source);
    let (none, _) = load(root, &NoPolicy);
    assert_eq!(effective.settings(), none.settings());
    assert_eq!(effective.warnings(), none.warnings());
    assert!(
        effective.lock_reasons().is_empty(),
        "{:?}",
        effective.lock_reasons()
    );
    assert!(!effective.is_locked("ui.theme"));
    assert!(!effective.policy_present());
    assert!(effective.policy().is_absent());
    assert!(effective.policy().errors().is_empty());
    assert_eq!(log.matches("a policy file is ignored").count(), 1, "{log}");
    assert!(log.contains(label), "{log}");
    assert!(log.contains(reason), "{log}");
    assert!(!log.contains(&*root.to_string_lossy()), "{log}");
}

/// A source with no policy.
struct NoPolicy;

impl PolicySource for NoPolicy {
    fn name(&self) -> &'static str {
        "none"
    }

    fn read(&self) -> clauseo_settings::PolicyRead {
        clauseo_settings::PolicyRead::Absent
    }
}

#[cfg(unix)]
mod unix {
    use super::{POLICY, assert_ignored, load, own_flag};
    use crate::write;
    use clauseo_settings::policy::TrustedFilePolicy;
    use clauseo_settings::{PolicyRead, PolicySource, Theme};
    use std::fs::{self, Permissions};
    use std::io;
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    use std::path::Path;
    use std::path::PathBuf;

    /// `source`'s file is read: its value applies and locks, and nothing is logged.
    fn assert_read(root: &Path, source: &dyn PolicySource) {
        let (effective, log) = load(root, source);
        assert_eq!(effective.settings().ui.theme, Theme::Dark);
        assert!(effective.is_locked("ui.theme"));
        assert_eq!(effective.policy().applied(), [source.name()]);
        assert_eq!(log, "");
    }

    const LABEL: &str = "the system policy file";
    const OWNER: &str = "owner is not an administrator";
    const WRITABLE: &str = "writable by non-administrators";
    const LINK: &str = "is a link or reparse point";
    const UNUSABLE: &str = "cannot be opened, or is not a folder or plain file";

    /// A fresh `<root>/clauseo/policy.json` holding [`POLICY`], with these modes, and the test
    /// user's id.
    struct Planted {
        root: PathBuf,
        folder: PathBuf,
        file: PathBuf,
        owner: u32,
    }

    fn chmod(path: &Path, mode: u32) -> io::Result<()> {
        write::set_permissions(path, Permissions::from_mode(mode))
    }

    fn plant(test: &str, folder_mode: u32, file_mode: u32) -> io::Result<Planted> {
        let root = crate::fresh(&format!("trust-{test}"))?;
        chmod(&root, 0o755)?;
        let folder = root.join("clauseo");
        write::fresh_dir(&folder)?;
        let file = folder.join("policy.json");
        write::write(&file, POLICY)?;
        chmod(&file, file_mode)?;
        chmod(&folder, folder_mode)?;
        let owner = fs::metadata(&root)?.uid();
        Ok(Planted {
            root,
            folder,
            file,
            owner,
        })
    }

    impl Planted {
        fn source(&self) -> TrustedFilePolicy {
            source(self.file.clone(), self.owner)
        }
    }

    /// The policy at `path`, expecting `owner`, with its own warn-once flag.
    fn source(path: PathBuf, owner: u32) -> TrustedFilePolicy {
        TrustedFilePolicy::owned_by(path, owner).warn_once_with(own_flag())
    }

    #[test]
    fn a_file_the_expected_owner_owns_with_no_group_or_other_write_is_read() {
        let planted = plant("trusted", 0o755, 0o644).unwrap();
        assert_read(&planted.root, &planted.source());
        assert_read(&planted.root, &planted.source().with_parent_check());
        let private = plant("trusted-private", 0o700, 0o600).unwrap();
        assert_read(&private.root, &private.source());
    }

    #[test]
    fn a_group_or_world_writable_file_is_ignored() {
        for (name, mode) in [("group-file", 0o664), ("world-file", 0o646)] {
            let planted = plant(name, 0o755, mode).unwrap();
            assert_ignored(&planted.root, &planted.source(), LABEL, WRITABLE);
        }
    }

    #[test]
    fn a_group_or_world_writable_folder_is_ignored() {
        for (name, mode) in [("group-folder", 0o775), ("world-folder", 0o757)] {
            let planted = plant(name, mode, 0o644).unwrap();
            assert_ignored(&planted.root, &planted.source(), LABEL, WRITABLE);
        }
    }

    #[test]
    fn a_writable_parent_folder_is_ignored_only_with_the_parent_check() {
        let planted = plant("writable-parent", 0o755, 0o644).unwrap();
        chmod(&planted.root, 0o777).unwrap();
        assert_ignored(
            &planted.root,
            &planted.source().with_parent_check(),
            LABEL,
            WRITABLE,
        );
        // macOS's rule: `/Library/Application Support` is not checked.
        assert_read(&planted.root, &planted.source());
    }

    #[test]
    fn a_file_another_user_owns_is_ignored() {
        let planted = plant("other-owner", 0o755, 0o644).unwrap();
        let other = source(planted.file.clone(), planted.owner + 1);
        assert_ignored(&planted.root, &other, LABEL, OWNER);
    }

    #[test]
    fn a_symlinked_file_or_folder_is_ignored() {
        let planted = plant("symlink-file", 0o755, 0o644).unwrap();
        let real = planted.root.join("real.json");
        write::write(&real, POLICY).unwrap();
        chmod(&real, 0o644).unwrap();
        let link = planted.folder.join("link.json");
        write::symlink(&real, &link).unwrap();
        let link_source = source(link, planted.owner);
        assert_ignored(&planted.root, &link_source, LABEL, LINK);

        let planted = plant("symlink-folder", 0o755, 0o644).unwrap();
        let linked = planted.root.join("linked");
        write::symlink(&planted.folder, &linked).unwrap();
        let linked_source = source(linked.join("policy.json"), planted.owner);
        assert_ignored(&planted.root, &linked_source, LABEL, LINK);
    }

    #[test]
    fn a_trusted_file_with_a_second_name_elsewhere_is_still_read() {
        // A user can add a name in a folder of their own (macOS has no protected hard links);
        // only root can add one inside the policy folder, so a second name changes nothing.
        let planted = plant("hard-link", 0o755, 0o644).unwrap();
        write::hard_link(&planted.file, &planted.root.join("other-name.json")).unwrap();
        assert_eq!(fs::metadata(&planted.file).unwrap().nlink(), 2);
        assert_read(&planted.root, &planted.source());
    }

    #[test]
    fn an_ignored_file_is_logged_once_per_flag_not_once_per_read() {
        let planted = plant("once", 0o757, 0o644).unwrap();
        let source = planted.source();
        let (_, first) = load(&planted.root, &source);
        assert_eq!(
            first.matches("a policy file is ignored").count(),
            1,
            "{first}"
        );
        let (effective, second) = load(&planted.root, &source);
        assert!(!effective.policy_present());
        assert_eq!(second, "");
    }

    #[test]
    fn an_untrusted_file_that_does_not_parse_is_ignored_not_unreadable() {
        let planted = plant("unparsable", 0o757, 0o644).unwrap();
        write::write(&planted.file, b"{ not json").unwrap();
        assert_ignored(&planted.root, &planted.source(), LABEL, WRITABLE);
        // The same file, trusted, is unreadable: it locks the security keys.
        chmod(&planted.folder, 0o755).unwrap();
        assert!(matches!(planted.source().read(), PolicyRead::Unreadable(_)));
    }

    #[test]
    fn an_unreadable_file_in_an_untrusted_folder_is_ignored_not_unreadable() {
        // The folder is judged before the file is opened.
        let planted = plant("unreadable-in-untrusted", 0o757, 0o000).unwrap();
        assert_ignored(&planted.root, &planted.source(), LABEL, WRITABLE);
    }

    /// Gives a folder back its mode, even when the test fails, so it can be removed.
    struct Reopen<'a>(&'a Path);

    impl Drop for Reopen<'_> {
        fn drop(&mut self) {
            let _reopened = chmod(self.0, 0o755);
        }
    }

    #[test]
    fn another_owners_folder_that_cannot_be_opened_is_ignored_not_unreadable() {
        let planted = plant("closed-folder", 0o755, 0o644).unwrap();
        chmod(&planted.folder, 0o000).unwrap();
        let _reopen = Reopen(&planted.folder);
        // Another owner's folder: `lstat` (or, for root, `fstat`) proves it planted.
        let other = source(planted.file.clone(), planted.owner + 1);
        assert_ignored(&planted.root, &other, LABEL, OWNER);
    }

    #[test]
    fn a_trusted_folder_this_process_cannot_open_is_unreadable_not_ignored() {
        // The expected owner's folder, with no group or other write, that this process cannot
        // open: as under its own sandbox (`sandbox-exec`, Landlock). Ignoring it would let a
        // process switch the policy off for itself, so it fails closed.
        let planted = plant("own-closed-folder", 0o755, 0o644).unwrap();
        if planted.owner == 0 {
            // Root opens any folder.
            return;
        }
        chmod(&planted.folder, 0o000).unwrap();
        let _reopen = Reopen(&planted.folder);
        assert!(matches!(planted.source().read(), PolicyRead::Unreadable(_)));
        let (effective, log) = load(&planted.root, &planted.source());
        assert!(effective.policy_present());
        assert!(!effective.settings().tools.web_search);
        assert_eq!(log, "");
    }

    #[test]
    fn a_file_where_the_folder_should_be_is_ignored_not_unreadable() {
        let planted = plant("file-for-folder", 0o755, 0o644).unwrap();
        let in_file = source(planted.file.join("policy.json"), planted.owner);
        assert_ignored(&planted.root, &in_file, LABEL, UNUSABLE);
    }

    #[test]
    fn a_file_that_cannot_be_opened_in_a_trusted_folder_is_unreadable() {
        // Only root could have put it there: the firm's policy is not dropped, the security
        // keys lock.
        let planted = plant("unreadable-in-trusted", 0o755, 0o000).unwrap();
        if planted.owner == 0 {
            // Root reads a 0o000 file.
            return;
        }
        assert!(matches!(planted.source().read(), PolicyRead::Unreadable(_)));
        let (effective, log) = load(&planted.root, &planted.source());
        assert!(effective.policy_present());
        assert!(!effective.settings().tools.web_search);
        assert_eq!(log, "");
    }

    #[test]
    fn no_file_or_no_folder_is_absent_with_nothing_logged() {
        // A trusted folder: an untrusted one is judged first, and ignored.
        let planted = plant("missing", 0o755, 0o644).unwrap();
        let missing = source(planted.folder.join("other.json"), planted.owner);
        let (effective, log) = load(&planted.root, &missing);
        assert!(!effective.policy_present());
        assert_eq!(log, "");
        let no_folder = source(
            planted.root.join("absent").join("policy.json"),
            planted.owner,
        );
        assert!(matches!(no_folder.read(), PolicyRead::Absent));
    }
}

#[cfg(windows)]
#[test]
fn a_policy_file_in_a_folder_the_test_user_made_is_ignored() {
    use clauseo_settings::policy::TrustedFilePolicy;
    use clauseo_settings::{Files as _, PolicyRead};
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("settings-trust-windows-{}", std::process::id()));
    let file = root.join("clauseo").join("policy.json");
    let source = || TrustedFilePolicy::admin_owned(file.clone()).warn_once_with(own_flag());
    assert!(matches!(source().read(), PolicyRead::Absent));
    SystemFiles.write_atomic(&file, POLICY).unwrap();
    // A non-elevated test user owns it; an elevated one (CI) makes Administrators the owner,
    // and the file then inherits the user's or the drive's write rights. Either fails.
    let (_, log) = load(&root, &source());
    let reason = if log.contains("owner is not an administrator") {
        "owner is not an administrator"
    } else {
        "writable by non-administrators"
    };
    assert_ignored(&root, &source(), "the ProgramData policy file", reason);
}
