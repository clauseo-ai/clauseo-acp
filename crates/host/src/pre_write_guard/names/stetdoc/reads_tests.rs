//! The stetdoc read fence on a disk in memory: what Claude's `Bash(stetdoc <sub>:*)` rules
//! may read, in Bash's and PowerShell's forms.

use super::{GuardFence, MATTERS_BUDGET, OVERFLOW_FLAG, msys_drive};

fn fence() -> GuardFence {
    GuardFence {
        data: vec!["/d".into()],
        own: vec!["/d/run/s-1".into()],
        matters: vec!["/w/b".into()],
        protected: vec!["/u/Documents/PowerShell".into()],
        overflow: false,
    }
}

/// The fence on a disk in memory, with Unix paths.
#[cfg(unix)]
mod on_disk {
    use std::path::{Path, PathBuf};

    use super::super::{Fenced, fenced_read};
    use super::fence;
    use crate::network_paths::resolve::Disk;
    use crate::network_paths::resolve::tests::{Fake, FakeFs};
    use crate::network_paths::tests::plain_net;
    use crate::pre_write_guard::words::Escape;

    const BASH: Escape = Escape::Backslash;
    const PWSH: Escape = Escape::Backtick;

    /// The matter `/w/m` (a link `link` to the other matter `/w/b`, a link it can't read, a name it
    /// may not look at), `/w/b`'s document also under its short name `/w/B~1`, the lawyer's
    /// Documents with a protected root in it, and `<data>` `/d` with the session's run folder.
    fn disk() -> FakeFs {
        FakeFs::with(&[
            ("/w/m/Contract.docx", Fake::File),
            ("/w/m/link", Fake::Link("/w/b".into())),
            ("/w/m/bad", Fake::BadLink),
            ("/w/m/locked", Fake::Denied),
            ("/w/b/secret.docx", Fake::File),
            ("/w/B~1/secret.docx", Fake::File),
            ("/u/Documents/template.docx", Fake::File),
            ("/u/Documents/PowerShell/profile.docx", Fake::File),
            ("/d/agent-state/m-2/notes/n.docx", Fake::File),
            ("/d/run/s-1/work/draft.docx", Fake::File),
        ])
    }

    /// Windows' long name for a short one: `/w/B~1` is `/w/b`.
    fn long_name(path: &Path) -> Option<PathBuf> {
        let rest = path.strip_prefix("/w/B~1").ok()?;
        Some(Path::new("/w/b").join(rest))
    }

    /// The fence's verdict on `command` in the shell of `escape`, from the matter `/w/m`.
    fn verdict(command: &str, escape: Escape, fence: &GuardFence) -> Option<Fenced> {
        let fs = disk();
        let net = plain_net(false);
        let disk = Disk {
            fs: &fs,
            real: &long_name,
            net: &net,
        };
        let matter = Path::new("/w/m");
        fenced_read(command, escape, (matter, matter), fence, &disk)
    }

    /// Each command in both shells (PowerShell's with `\` separators where relative).
    fn both(commands: &[&str], want: impl Fn(&str, Option<Fenced>) -> bool) {
        for command in commands {
            for (escape, text) in [
                (BASH, (*command).to_owned()),
                (PWSH, command.replace('/', "\\")),
            ] {
                let got = verdict(&text, escape, &fence());
                assert!(want(&text, got.clone()), "{escape:?} {text}: {got:?}");
            }
        }
    }

    fn matter_b(got: Option<Fenced>) -> bool {
        matches!(got, Some(Fenced::Matter(_, folder)) if folder == Path::new("/w/b"))
    }

    /// Another matter's document is refused through a link in this matter, under its short name,
    /// and in every form the allow rules match.
    #[test]
    fn another_matter_is_refused_through_a_link_or_a_short_name() {
        both(
            &[
                "stetdoc view link/secret.docx",
                "stetdoc view /w/B~1/secret.docx",
                "stetdoc view /w/b/secret.docx",
                "stetdoc --json view --find x --all link/secret.docx",
                "stetdoc check link/secret.docx --author Jane",
                "stetdoc inspect /w/b/secret.docx styles",
                "stetdoc inspect Contract.docx /w/b/secret.docx",
                "ls; stetdoc lookup /w/B~1/secret.docx Losses",
                "PATH=/x stetdoc view -- link/secret.docx",
            ],
            |_, got| matter_b(got),
        );
        for script in [
            "bash -c 'stetdoc view /w/b/secret.docx'",
            "pwsh -Command stetdoc view /w/b/secret.docx",
        ] {
            assert!(matter_b(verdict(script, BASH, &fence())), "{script}");
        }
    }

    /// A template in the lawyer's Documents, and the matter's own documents, read as before; so
    /// does a command that names no document.
    #[test]
    fn documents_and_the_matter_read_as_before() {
        both(
            &[
                "stetdoc view /u/Documents/template.docx",
                "stetdoc view Contract.docx",
                "stetdoc check /w/m/Contract.docx --all",
                "stetdoc lookup Contract.docx Losses",
                "stetdoc view /d/run/s-1/work/draft.docx",
                "stetdoc capabilities",
                "stetdoc view --help",
                "stetdoc --version",
                "cat /w/b/secret.docx",
            ],
            |_, got| got.is_none(),
        );
    }

    /// The data folder outside the own folders, and a protected root, are refused plainly.
    #[test]
    fn the_data_folder_and_a_protected_root_are_refused() {
        both(
            &["stetdoc view /d/agent-state/m-2/notes/n.docx"],
            |_, got| matches!(got, Some(Fenced::Data(_))),
        );
        both(
            &["stetdoc view /u/Documents/PowerShell/profile.docx"],
            |_, got| matches!(got, Some(Fenced::Protected(_))),
        );
    }

    /// An input it can't follow is refused: a link it can't read, a name it may not look at, a word
    /// the shell changes first, a relative one after a folder change.
    #[test]
    fn an_input_it_cannot_follow_is_refused() {
        both(
            &[
                "stetdoc view bad/x.docx",
                "stetdoc view locked/x.docx",
                "stetdoc view $HOME/x.docx",
                "stetdoc view ~/x.docx",
                "cd /w/b; stetdoc view secret.docx",
            ],
            |_, got| matches!(got, Some(Fenced::Unchecked(_))),
        );
        for command in ["stetdoc view /w/b/*.docx", "stetdoc view `pwd`/x.docx"] {
            let got = verdict(command, BASH, &fence());
            assert!(
                matches!(got, Some(Fenced::Unchecked(_))),
                "{command}: {got:?}"
            );
        }
        let quoted = verdict("stetdoc view '/u/Documents/template.docx'", BASH, &fence());
        assert_eq!(quoted, None);
    }

    /// A matter that holds this one fences only what lies outside this one; a cut-short fence lets
    /// stetdoc read only in the matter and the own folders; an empty one fences nothing.
    #[test]
    fn an_outer_matter_a_cut_short_fence_and_none() {
        let outer = GuardFence {
            matters: vec!["/w".into()],
            ..fence()
        };
        assert_eq!(verdict("stetdoc view Contract.docx", BASH, &outer), None);
        let got = verdict("stetdoc view /w/b/secret.docx", BASH, &outer);
        assert!(matches!(got, Some(Fenced::Matter(_, folder)) if folder == Path::new("/w")));
        let short = GuardFence {
            matters: Vec::new(),
            overflow: true,
            ..fence()
        };
        let template = "stetdoc view /u/Documents/template.docx";
        assert!(matches!(
            verdict(template, BASH, &short),
            Some(Fenced::Overflow(_))
        ));
        assert_eq!(verdict("stetdoc view Contract.docx", BASH, &short), None);
        assert_eq!(
            verdict("stetdoc view /d/run/s-1/work/draft.docx", BASH, &short),
            None
        );
        let none = GuardFence::default();
        assert_eq!(verdict("stetdoc view /w/b/secret.docx", BASH, &none), None);
    }
}

/// The fence goes to the guard as arguments and comes back whole; past the budget the matters
/// are left out and the guard told so.
#[test]
fn the_fence_goes_as_arguments() {
    let fence = fence();
    let args: Vec<std::ffi::OsString> = fence.args().into_iter().map(Into::into).collect();
    assert_eq!(GuardFence::from_args(&args), fence);
    let many = GuardFence {
        matters: (0..MATTERS_BUDGET / 10)
            .map(|n| format!("/w/{n:08}").into())
            .collect(),
        ..GuardFence::default()
    };
    let args = many.args();
    assert_eq!(args, [OVERFLOW_FLAG]);
}

/// Git Bash's drive form.
#[test]
fn git_bash_drives_become_windows_paths() {
    assert_eq!(
        msys_drive("/c/Users/l/x.docx").as_deref(),
        Some(r"C:\Users\l\x.docx")
    );
    assert_eq!(msys_drive("/d").as_deref(), Some(r"D:\"));
    assert_eq!(msys_drive("/cc/x"), None);
    assert_eq!(msys_drive("c/x"), None);
}
