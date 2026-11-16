//! The stetdoc read fence on a disk in memory: what Claude's `Bash(stetdoc <sub>:*)` rules
//! may read, in Bash's and PowerShell's forms.

use super::{GuardFence, MATTERS_BUDGET, OVERFLOW_FLAG, git_bash};

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
    use super::{GuardFence, fence};
    use crate::network_paths::resolve::Disk;
    use crate::network_paths::resolve::tests::{Fake, FakeFs};
    use crate::network_paths::tests::{mac_net, plain_net};
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
        matches!(got, Some(Fenced::Matter(_)))
    }

    /// No short names on the Mac.
    fn no_long_name(_: &Path) -> Option<PathBuf> {
        None
    }

    /// On the Mac the data folder spelled on the data volume (`/System/Volumes/Data/Users/…`)
    /// is its firmlinked spelling (`/Users/…`), whichever way round the fence and the read are.
    #[test]
    fn the_data_volume_spelling_of_the_data_folder_is_refused() {
        let fs = FakeFs::with(&[("/Users/a/d/logs/x.docx", Fake::File)]);
        let net = mac_net(&[]);
        let disk = Disk {
            fs: &fs,
            real: &no_long_name,
            net: &net,
        };
        let matter = Path::new("/Users/a/m");
        for (data, read) in [
            ("/Users/a/d", "/System/Volumes/Data/Users/a/d/logs/x.docx"),
            ("/System/Volumes/Data/Users/a/d", "/Users/a/d/logs/x.docx"),
            (
                "/System/Volumes/Data/Users/a/d",
                "/system/volumes/data/users/a/d/logs/x.docx",
            ),
        ] {
            let fence = GuardFence {
                data: vec![data.into()],
                ..GuardFence::default()
            };
            let command = format!("stetdoc view {read}");
            let got = fenced_read(&command, BASH, (matter, matter), &fence, &disk);
            assert!(
                matches!(got, Some(Fenced::Data(_))),
                "{data} {read}: {got:?}"
            );
        }
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
                "stetdoc view -- link/secret.docx",
                "stetdoc view --find=/w/b/secret.docx Contract.docx",
                "stetdoc view Contract.docx | head -5; stetdoc view link/secret.docx",
            ],
            |_, got| matter_b(got),
        );
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
                "ls /w/b/*.docx",
                "stetdoc view Contract.docx 2>&1 | head -50",
                "stetdoc view 'Contract.docx' && stetdoc check Contract.docx",
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

    /// An input it can't follow is refused: a link it can't read, a name it may not look at.
    /// A document that isn't there is refused as missing.
    #[test]
    fn an_input_it_cannot_follow_is_refused() {
        both(
            &["stetdoc view bad/x.docx", "stetdoc view locked/x.docx"],
            |_, got| matches!(got, Some(Fenced::Unchecked(_))),
        );
        both(&["stetdoc view new.docx"], |_, got| {
            matches!(got, Some(Fenced::Missing(_)))
        });
        let quoted = verdict("stetdoc view '/u/Documents/template.docx'", BASH, &fence());
        assert_eq!(quoted, None);
    }

    /// Each command, in the shell of `escape`, is refused as not the plain shape.
    fn not_plain(escape: Escape, commands: &[&str]) {
        for command in commands {
            let got = verdict(command, escape, &fence());
            assert_eq!(got, Some(Fenced::Shape), "{escape:?} {command}");
        }
    }

    /// A line past the shape's length, with a stetdoc read after the limit, is refused.
    #[test]
    fn a_long_stetdoc_line_is_refused() {
        let filler = "a ".repeat(1100);
        not_plain(
            BASH,
            &[
                &format!("stetdoc capabilities {filler}; stetdoc view /w/b/secret.docx"),
                &format!("echo {filler}; stetdoc view /w/b/secret.docx"),
            ],
        );
    }

    /// A stetdoc line with a wildcard anywhere, quoted or escaped in part or not, is refused.
    #[test]
    fn a_stetdoc_line_with_a_wildcard_is_refused() {
        not_plain(
            BASH,
            &[
                "stetdoc view /w/\\b*/secret.docx",
                "stetdoc view \"/w/\"[b]/secret.docx",
                "stetdoc view /\\d?/agent-state/m-2/notes/n.docx",
                "stetdoc view /w/(b)/secret.docx",
                "stetdoc view '/w/b?/secret.docx'",
                "stetdoc view /w/b\\ x/sec*.docx",
                "stetdoc view /w/b/*.docx",
            ],
        );
        not_plain(
            PWSH,
            &[
                "stetdoc view /w/b/*.docx",
                "stetdoc view '/w/b/secret.doc?'",
            ],
        );
    }

    /// A stetdoc line that changes folder, sets a variable, or defines an alias or a function,
    /// however it is written, is refused.
    #[test]
    fn a_stetdoc_line_with_a_folder_change_or_a_definition_is_refused() {
        not_plain(
            BASH,
            &[
                "cd /w/b; stetdoc view secret.docx",
                "pushd /w/b && stetdoc view secret.docx",
                "X=cd; $X /w/b; stetdoc view secret.docx",
                "alias c=cd\nc /w/b\nstetdoc view secret.docx",
                "f() { cd /w/b; }; f; stetdoc view secret.docx",
                "PATH=/x stetdoc view Contract.docx",
                "stetdoc view $HOME/x.docx",
                "stetdoc view ~/x.docx",
                "stetdoc view `pwd`/x.docx",
            ],
        );
        not_plain(
            PWSH,
            &[
                "Set-Location /w/b; stetdoc view secret.docx",
                "sl /w/b; stetdoc view secret.docx",
                "Push-Location /w/b; stetdoc view secret.docx",
                "Set-Alias c Set-Location; c /w/b; stetdoc view secret.docx",
                "New-Variable x /w/b; stetdoc view secret.docx",
            ],
        );
    }

    /// A stetdoc line that makes a link is refused; the link's name read alone is missing.
    #[test]
    fn a_stetdoc_line_that_makes_a_link_is_refused() {
        not_plain(
            BASH,
            &["ln -s /w/b/secret.docx new.docx; stetdoc view new.docx"],
        );
        not_plain(
            PWSH,
            &[
                "New-Item -ItemType SymbolicLink -Path new.docx -Target /w/b/secret.docx; stetdoc view new.docx",
                "cmd /c mklink new.docx /w/b/secret.docx; stetdoc view new.docx",
            ],
        );
    }

    /// A stetdoc command run through another program, or a copy of it, is refused.
    #[test]
    fn a_stetdoc_line_with_a_wrapper_is_refused() {
        not_plain(
            BASH,
            &[
                "env stetdoc view /w/b/secret.docx",
                "nice stetdoc view /w/b/secret.docx",
                "exec stetdoc view /w/b/secret.docx",
                "command stetdoc view /w/b/secret.docx",
                "time stetdoc view /w/b/secret.docx",
                "echo /w/b/secret.docx | xargs stetdoc view",
                "find /w/b -name x.docx -exec stetdoc view {} ;",
                "bash -c \"bash -c 'stetdoc view /w/b/secret.docx'\"",
                "sh <<EOF\nstetdoc view /w/b/secret.docx\nEOF",
                "cmd /c stetdoc view /w/b/secret.docx",
                "cp /bin/stetdoc sd; ./sd view /w/b/secret.docx",
                "./stetdoc view /w/b/secret.docx",
                "STETDOC view /w/b/secret.docx",
            ],
        );
        not_plain(
            PWSH,
            &[
                "Start-Process stetdoc -ArgumentList view,/w/b/secret.docx",
                "Start-Process stetdoc view",
                "cmd /c stetdoc view /w/b/secret.docx",
                "iex 'stetdoc view /w/b/secret.docx'",
                "& stetdoc view /w/b/secret.docx",
            ],
        );
    }

    /// A stetdoc command fed by a pipe, a redirection or a process substitution is refused, and
    /// so is the standard input word.
    #[test]
    fn a_stetdoc_command_with_redirected_input_is_refused() {
        not_plain(
            BASH,
            &[
                "cat /w/b/secret.docx | stetdoc view /dev/stdin",
                "stetdoc view <(cat /w/b/secret.docx)",
                "stetdoc view /dev/stdin < /w/b/secret.docx",
                "stetdoc view - < /w/b/secret.docx",
                "stetdoc view -",
                "stetdoc view -o/w/b Contract.docx",
            ],
        );
    }

    /// PowerShell's own forms in a stetdoc line are refused.
    #[test]
    fn a_stetdoc_line_in_powershells_own_syntax_is_refused() {
        not_plain(
            PWSH,
            &[
                "$a='/w/b/secret.docx'; stetdoc view @a",
                "stetdoc --% view /w/b/secret.docx",
                "stetdoc view (Join-Path /w/b secret.docx)",
                "stetdoc view \"/`w/b/secret.docx\"",
                "stetdoc view @'\n/w/b/secret.docx\n'@",
                "stetdoc view /w/b/secret.docx,Contract.docx",
                "stetdoc view \u{2018}/w/b/secret.docx\u{2019}",
                "stetdoc view $env:X",
            ],
        );
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
        assert!(matches!(got, Some(Fenced::Matter(_))));
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

/// A UNC path is refused before the disk is looked at.
#[cfg(windows)]
mod unc {
    use std::io;
    use std::path::{Path, PathBuf};

    use super::super::{Fenced, fenced_read};
    use super::fence;
    use crate::network_paths::resolve::{Disk, Entry, Fs};
    use crate::network_paths::tests::plain_net;
    use crate::pre_write_guard::words::Escape;

    /// A disk that fails the test when a server's path is looked at.
    struct NoServer;

    impl Fs for NoServer {
        fn entry(&self, path: &Path) -> io::Result<Entry> {
            looked_at(path)
        }
        fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
            looked_at(path)
        }
        fn identity(&self, path: &Path) -> io::Result<(u64, u128)> {
            looked_at(path)
        }
    }

    fn looked_at<T>(path: &Path) -> io::Result<T> {
        let text = path.to_string_lossy().to_lowercase();
        assert!(!text.contains("server"), "looked at {}", path.display());
        Err(io::ErrorKind::NotFound.into())
    }

    fn no_long_name(_: &Path) -> Option<PathBuf> {
        None
    }

    #[test]
    fn a_unc_path_is_refused_without_looking_at_the_disk() {
        let net = plain_net(true);
        let disk = Disk {
            fs: &NoServer,
            real: &no_long_name,
            net: &net,
        };
        let matter = Path::new(r"C:\w\m");
        for (escape, command) in [
            (Escape::Backtick, r"stetdoc view \\server\share\x.docx"),
            (Escape::Backtick, r"stetdoc view '\\server\share\x.docx'"),
            (
                Escape::Backtick,
                r"stetdoc view '\\?\UNC\server\share\x.docx'",
            ),
            (Escape::Backslash, "stetdoc view //server/share/x.docx"),
        ] {
            let got = fenced_read(command, escape, (matter, matter), &fence(), &disk);
            assert!(
                matches!(got, Some(Fenced::Unchecked(_) | Fenced::Shape)),
                "{command}: {got:?}"
            );
        }
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

/// Git Bash's drive forms become Windows paths; its other root paths, path lists and stray
/// colons are not read.
#[test]
fn git_bash_forms_become_windows_paths_or_none() {
    for (word, want) in [
        ("/c/Users/l/x.docx", r"C:\Users\l\x.docx"),
        ("/d", r"D:\"),
        ("/cygdrive/c/x.docx", r"C:\x.docx"),
        ("/proc/cygdrive/e/m/x.docx", r"E:\m\x.docx"),
        ("C:/m/x.docx", "C:/m/x.docx"),
        ("sub/x.docx", "sub/x.docx"),
    ] {
        assert_eq!(git_bash(word).as_deref(), Some(want), "{word}");
    }
    for word in [
        "/tmp/x.docx",
        "/usr/x",
        "//server/s/x.docx",
        "/cc/x",
        "/c:/x",
        "/c/a:/c/b",
        "C:/a;D:/b",
    ] {
        assert_eq!(git_bash(word), None, "{word}");
    }
}
