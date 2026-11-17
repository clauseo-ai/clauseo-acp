use std::path::{Path, PathBuf};

use serde_json::json;

use super::super::input::{command_args, from_command_cwd};
use super::super::tests::{check, layout};
use super::super::{FenceCheck, PathsChecked, check_paths};
use super::OtherMatters;
use crate::write;

/// A Codex request running `command` in the matter folder, with `Other` (beside it) as another
/// matter, as `actor/turn/asks.rs` builds it: [`check`]'s paths, and the command's words as
/// [`OtherMatters::args`].
fn codex(root: &Path, command: &str) -> FenceCheck {
    codex_in(root, command, &root.join("matter"))
}

/// [`codex`] with the command run in `cwd` (`rawInput.cwd`).
fn codex_in(root: &Path, command: &str, cwd: &Path) -> FenceCheck {
    let input = json!({"command": command, "cwd": cwd.display().to_string()});
    let mut fence = check(root, &[], Some(&input));
    from_command_cwd(Some(&input), &mut fence.paths);
    from_command_cwd(Some(&input), &mut fence.roots);
    fence.other_matters = Some(OtherMatters {
        own: root.join("matter"),
        others: vec![root.join("Other")],
        args: command_args(Some(&input), cfg!(windows)),
        cache: super::Cache::default(),
    });
    fence
}

/// [`codex`] with `others` as the other matters' folders.
fn codex_with(root: &Path, command: &str, others: Vec<PathBuf>) -> FenceCheck {
    let mut fence = codex(root, command);
    if let Some(matters) = fence.other_matters.as_mut() {
        matters.others = others;
    }
    fence
}

/// What the matter thread answers for `fence`.
fn checked(root: &Path, fence: &FenceCheck) -> &'static str {
    match check_paths(&root.join("matter"), fence.paths.clone(), Some(fence)) {
        PathsChecked::DataFolder => "data",
        PathsChecked::OtherMatter => "other",
        PathsChecked::Held(_) => "card",
    }
}

/// [`layout`] with another matter's folder `Other` beside the matter, holding `brief.txt`.
fn with_other(test: &str) -> crate::pipeline::tests::support::TempDir {
    let dir = layout(test);
    write::create_dir_all(&dir.path().join("Other")).unwrap();
    write::write_file(&dir.path().join("Other/brief.txt"), b"other").unwrap();
    dir
}

/// `cp ../Other/brief.txt out.txt`, relative to the command's folder: refused before the card.
#[test]
fn a_relative_path_into_another_matter_is_refused() {
    let dir = with_other("other-relative");
    let root = dir.path();
    let fence = codex(root, "cp ../Other/brief.txt out.txt");
    assert_eq!(checked(root, &fence), "other");
}

/// A link in the matter folder whose target was swapped into another matter: the bare name
/// `notes.txt` is followed through the link.
#[cfg(unix)]
#[test]
fn a_link_swapped_into_another_matter_is_refused() {
    let dir = with_other("other-link");
    let root = dir.path();
    write::symlink(
        &root.join("Other/brief.txt"),
        &root.join("matter/notes.txt"),
    )
    .unwrap();
    let fence = codex(root, "cat notes.txt");
    assert_eq!(checked(root, &fence), "other");
}

/// A bare name in the command's folder that leads into the data folder is refused by the
/// pre-card check (the same refusal as the data fence's); a bare name into the session's own
/// run folder there is not.
#[cfg(unix)]
#[test]
fn a_bare_name_into_the_data_folder_is_refused() {
    let dir = with_other("other-data");
    let root = dir.path();
    write::symlink(&root.join("data/skill-packs"), &root.join("matter/packs")).unwrap();
    write::symlink(&root.join("data/run/s1/work"), &root.join("matter/work")).unwrap();
    assert_eq!(checked(root, &codex(root, "cat packs")), "other");
    assert_eq!(checked(root, &codex(root, "ls work")), "card");
}

/// A built-in skill's script in the shared plugin folder (`<data>/profile`, `SHARED`), named by
/// its absolute path as Codex's skill index gives it, still reaches the card.
#[test]
fn a_built_in_skill_path_goes_to_the_card() {
    let dir = with_other("other-profile");
    let root = dir.path();
    let skill = root.join("data/profile/claude-plugin/skills/docx/scripts");
    write::create_dir_all(&skill).unwrap();
    write::write_file(&skill.join("unpack.py"), b"x").unwrap();
    let command = format!(
        "python3 \"{}\" contract.docx",
        skill.join("unpack.py").display()
    );
    assert_eq!(checked(root, &codex(root, &command)), "card");
}

/// `dd if=notes.txt`: a `name=value` word's value is followed through a swapped link.
#[cfg(unix)]
#[test]
fn a_name_value_word_is_followed() {
    let dir = with_other("other-dd");
    let root = dir.path();
    write::symlink(
        &root.join("Other/brief.txt"),
        &root.join("matter/notes.txt"),
    )
    .unwrap();
    assert_eq!(
        checked(root, &codex(root, "dd if=notes.txt of=out.txt")),
        "other"
    );
}

/// A search over the parent folder reaches every sibling matter: refused (here the data folder
/// beside it is reached first, so the pre-card check is asked directly too); one over this
/// matter is not.
#[test]
fn a_search_holding_another_matter_is_refused() {
    let dir = with_other("other-search");
    let root = dir.path();
    for command in ["grep -r secret ..", "find .. -name x"] {
        let fence = codex(root, command);
        assert_ne!(checked(root, &fence), "card", "{command}");
        assert!(reached(&fence), "{command}");
    }
    let fence = codex(root, "grep -r secret .");
    assert_eq!(checked(root, &fence), "card");
    assert!(!reached(&fence));
}

/// The command's bare words never reach the data-folder check's Windows name rules, where
/// `TODO:` (a name with a `:`) or `nul` would be refused: they stay in the pre-card check.
#[cfg(unix)]
#[test]
fn bare_words_are_not_judged_by_windows_names() {
    use super::super::candidates;
    use super::super::network_tests::{fence, refused};
    use crate::network_paths::resolve::tests::FakeFs;
    use crate::network_paths::tests::plain_net;
    for command in [r#"rg -n "TODO:" ."#, "git show HEAD:notes.txt", "dir 2>nul"] {
        let input = json!({"command": command, "cwd": "/u/m"});
        let paths: Vec<String> = candidates(&[], Some(&input), true)
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        let check = fence("/u/m", &paths);
        assert!(
            !refused(&check, &plain_net(true), &FakeFs::with(&[])),
            "{command}"
        );
    }
}

/// A plain command in the matter folder reaches the lawyer, and so does one outside every
/// matter (Codex's sandbox and the card decide that one).
#[test]
fn a_plain_in_matter_command_goes_to_the_card() {
    let dir = with_other("other-plain");
    let root = dir.path();
    for command in [
        "cp sub/a.txt sub/b.txt",
        "wc -l sub/a.txt",
        "touch ../outside/x.txt",
        "grep --include=*.txt -r clause .",
    ] {
        assert_eq!(checked(root, &codex(root, command)), "card", "{command}");
    }
}

/// A matter nested in this one's folder is another matter; this folder sitting inside another
/// matter's does not make its own files that matter's.
#[test]
fn nesting_counts_only_for_a_matter_inside_this_one() {
    let dir = with_other("other-nested");
    let root = dir.path();
    write::create_dir_all(&root.join("matter/inner")).unwrap();
    let fence = codex_with(
        root,
        "cat inner/x.txt",
        vec![root.join("matter/inner"), root.to_owned()],
    );
    assert_eq!(checked(root, &fence), "other");
    let fence = codex_with(root, "cat sub/a.txt", vec![root.to_owned()]);
    assert_eq!(checked(root, &fence), "card");
}

/// No other matters set (every agent but Codex): nothing is refused for it.
#[test]
fn without_other_matters_nothing_is_refused_for_them() {
    let dir = with_other("other-none");
    let root = dir.path();
    let mut fence = codex(root, "cp ../Other/brief.txt out.txt");
    fence.other_matters = None;
    assert_eq!(checked(root, &fence), "card");
}

/// The pre-card check asked directly, past the data-folder check.
fn reached(fence: &FenceCheck) -> bool {
    let net = crate::network_paths::NetEnv::current();
    let disk = super::super::resolve::Disk::real(&net);
    let matters = fence.other_matters.as_ref().unwrap();
    matters.reached(fence, crate::network_paths::FOLD_CASE, &disk)
}

/// Paths and search folders are taken from `rawInput.cwd`, not the matter folder: `grep -r .`
/// run in the folder above the matters is refused; `grep -r ..` run in a subfolder of the
/// matter searches only the matter and is not; a command run inside another matter is judged
/// there.
#[test]
fn paths_are_taken_from_the_commands_folder() {
    let dir = with_other("other-cwd");
    let root = dir.path();
    let fence = codex_in(root, "grep -r secret .", root);
    assert_ne!(checked(root, &fence), "card");
    assert!(reached(&fence));
    let fence = codex_in(root, "grep -r x ..", &root.join("matter/sub"));
    assert_eq!(checked(root, &fence), "card");
    let fence = codex_in(root, "cat brief.txt", &root.join("Other"));
    assert_eq!(checked(root, &fence), "other");
}

/// A bare name the walk can't vouch for (a link onto a network path) is refused; a word whose
/// own text is a network path is left to the data-folder check, which judged it.
#[cfg(unix)]
#[test]
fn a_bare_name_that_does_not_resolve_is_refused() {
    let dir = with_other("other-unresolved");
    let root = dir.path();
    write::symlink(
        Path::new("/net/server/brief.txt"),
        &root.join("matter/notes.txt"),
    )
    .unwrap();
    assert_eq!(checked(root, &codex(root, "cat notes.txt")), "other");
    assert!(!reached(&codex(root, r"printf '\\n'")));
}

/// With no `rawInput.cwd`, or a relative one, a name that is a link onto a network path is
/// still refused: it is looked at from the matter folder, not the host's own folder, and a
/// relative `cwd` (`sub`) is joined to the matter folder (the link is `matter/sub/notes.txt`).
#[cfg(unix)]
#[test]
fn a_bare_link_to_a_share_without_an_absolute_cwd_is_refused() {
    let dir = with_other("other-no-cwd");
    let root = dir.path();
    write::create_dir_all(&root.join("matter/sub")).unwrap();
    write::symlink(
        Path::new("/net/server/brief.txt"),
        &root.join("matter/sub/notes.txt"),
    )
    .unwrap();
    for input in [
        json!({"command": "cat sub/notes.txt"}),
        json!({"command": "cat notes.txt", "cwd": "sub"}),
    ] {
        let mut fence = check(root, &[], Some(&input));
        fence.other_matters = Some(OtherMatters {
            own: root.join("matter"),
            others: vec![root.join("Other")],
            args: command_args(Some(&input), cfg!(windows)),
            cache: super::Cache::default(),
        });
        assert!(reached(&fence), "{input}");
    }
}

/// The words taken: options and expansions are not, a `name=value`'s value is, each from
/// the command's folder.
#[test]
fn command_args_take_the_words_that_may_name_files() {
    let cwd = if cfg!(windows) { r"C:\m" } else { "/m" };
    let input = json!({"command": "cp -r --target-directory=out \"$HOME\" a*.txt https://x.in/a b",
        "cwd": cwd});
    let got: Vec<PathBuf> = command_args(Some(&input), cfg!(windows));
    let want: Vec<PathBuf> = ["cp", "out", "b"]
        .iter()
        .map(|word| Path::new(cwd).join(word))
        .collect();
    assert_eq!(got, want);
}

/// [`codex`] with this matter's folder spelled `own` and the other matters' `others`.
#[cfg(unix)]
fn codex_as(root: &Path, command: &str, own: PathBuf, others: Vec<PathBuf>) -> FenceCheck {
    let mut fence = codex_with(root, command, others);
    if let Some(matters) = fence.other_matters.as_mut() {
        matters.own = own;
    }
    fence
}

/// A path spelled for a shell, quoted.
#[cfg(unix)]
fn quoted(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

/// Spellings through a link, both ways: another matter registered through a link to its folder
/// and named by its real path, and registered by its real path and named through the link. A
/// comparison of the texts misses both; the check resolves both sides.
#[cfg(unix)]
#[test]
fn a_linked_spelling_of_another_matter_is_found_both_ways() {
    let dir = with_other("other-linked");
    let root = dir.path();
    write::symlink(&root.join("Other"), &root.join("other-link")).unwrap();
    let real = root.join("Other/brief.txt");
    let linked = root.join("other-link/brief.txt");
    let fence = codex_with(
        root,
        &format!("cat {}", quoted(&real)),
        vec![root.join("other-link")],
    );
    assert_eq!(checked(root, &fence), "other");
    let fence = codex_with(
        root,
        &format!("cat {}", quoted(&linked)),
        vec![root.join("Other")],
    );
    assert_eq!(checked(root, &fence), "other");
}

/// This matter registered through a link, its files named by the real path, inside a matter
/// that holds it: still this matter's own files, not the outer one's.
#[cfg(unix)]
#[test]
fn this_matter_through_a_link_is_still_its_own() {
    let dir = with_other("other-own-link");
    let root = dir.path();
    write::symlink(&root.join("matter"), &root.join("matter-link")).unwrap();
    let file = root.join("matter/sub/a.txt");
    let fence = codex_as(
        root,
        &format!("wc -l {}", quoted(&file)),
        root.join("matter-link"),
        vec![root.to_owned()],
    );
    assert_eq!(checked(root, &fence), "card");
}

/// On a volume that ignores case, another matter named in another case is found.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn another_case_of_another_matter_is_found() {
    let dir = with_other("other-case");
    let root = dir.path();
    assert_eq!(
        checked(root, &codex(root, "cp ../OTHER/brief.txt out.txt")),
        "other"
    );
}

/// macOS: a matter named in NFC (`Café`), named by the command in NFD; the volume ignores the
/// form, and so does the check.
#[cfg(target_os = "macos")]
#[test]
fn another_unicode_form_of_another_matter_is_found() {
    let dir = layout("other-nfd");
    let root = dir.path();
    // Spelled by its characters: `C`, `a`, `f`, then `é` as one (NFC) or `e` and a combining
    // acute accent (NFD).
    let name = |tail: &[char]| ['C', 'a', 'f'].iter().chain(tail).collect::<String>();
    let nfc = root.join(name(&['\u{e9}']));
    write::create_dir_all(&nfc).unwrap();
    write::write_file(&nfc.join("brief.txt"), b"x").unwrap();
    let command = format!("cat ../{}/brief.txt", name(&['e', '\u{301}']));
    let fence = codex_with(root, &command, vec![nfc]);
    assert_eq!(checked(root, &fence), "other");
}

/// macOS: `/System/Volumes/Data/…` (a firmlink, no link to follow) names the same folder as the
/// path without it; found by identity, both ways.
#[cfg(target_os = "macos")]
#[test]
fn a_firmlinked_spelling_of_another_matter_is_found_both_ways() {
    let dir = with_other("other-firmlink");
    let root = dir.path();
    let real = dunce::canonicalize(root.join("Other")).unwrap();
    let data = Path::new("/System/Volumes/Data").join(real.strip_prefix("/").unwrap());
    if !data.join("brief.txt").exists() {
        return;
    }
    let fence = codex_with(
        root,
        &format!("cat {}", quoted(&data.join("brief.txt"))),
        vec![root.join("Other")],
    );
    assert_eq!(checked(root, &fence), "other");
    let fence = codex_with(root, "cp ../Other/brief.txt out.txt", vec![data]);
    assert_eq!(checked(root, &fence), "other");
}

/// Windows: a word of one letter and a colon (`Select-String -Pattern "Q:"`, Python's
/// `except Exception as e:`) joined to the command's folder is drive-relative, which the walk
/// can't resolve; it names no file of another matter, so the command reaches the card.
#[cfg(windows)]
#[test]
fn a_drive_relative_word_reaches_the_card() {
    let dir = with_other("other-drive-word");
    let root = dir.path();
    for command in [
        r#"Select-String -Path depo.txt -Pattern "Q:""#,
        r#"findstr "A:" depo.txt"#,
        "python -c \"try: pass\nexcept Exception as e: pass\"",
        "python -c \"try:\n    pass\nexcept Exception as e:\n    pass\"",
    ] {
        let fence = codex(root, command);
        assert_eq!(checked(root, &fence), "card", "{command}");
        assert!(!reached(&fence), "{command}");
    }
}

/// A word whose walk fails on a link (one that loops) is refused, as one through a dangling
/// link into another matter is; one whose walk fails on the word itself (a name under a file,
/// a name under a folder that can't be read) is not refused by this check, and a missing
/// name never was.
#[cfg(unix)]
#[test]
fn only_a_walk_failing_on_a_link_is_refused() {
    let dir = with_other("other-narrow");
    let root = dir.path();
    let matter = root.join("matter");
    write::symlink(Path::new("loop"), &matter.join("loop")).unwrap();
    write::symlink(&root.join("Other/missing.txt"), &matter.join("dangle")).unwrap();
    write::write_file(&matter.join("contract.docx"), b"x").unwrap();
    write::create_dir_all(&matter.join("locked")).unwrap();
    for command in ["cat loop", "cat loop/x", "cat dangle"] {
        assert!(reached(&codex(root, command)), "{command}");
    }
    write::set_mode(&matter.join("locked"), 0o000).unwrap();
    let passed: Vec<bool> = [
        "ls contract.docx/word",
        "ls locked/in.txt",
        "touch brief-v2.docx",
    ]
    .iter()
    .map(|command| reached(&codex(root, command)))
    .collect();
    write::set_mode(&matter.join("locked"), 0o755).unwrap();
    assert_eq!(passed, [false, false, false]);
}

/// A variable at the start of a path (`$env:USERPROFILE\…`, `$HOME/…`, `%USERPROFILE%\…`) is
/// left as written, not taken from the command's folder.
#[test]
fn a_variable_path_is_not_taken_from_the_commands_folder() {
    let cwd = if cfg!(windows) { r"C:\cwd" } else { "/cwd" };
    let input = json!({"command": "Get-Content x", "cwd": cwd});
    let words = [
        r"$env:USERPROFILE\Documents\Clients\B\brief.txt",
        "$HOME/Documents/brief.txt",
        r"%USERPROFILE%\Documents\brief.txt",
        "brief.txt",
    ];
    let mut paths: Vec<PathBuf> = words.iter().map(PathBuf::from).collect();
    from_command_cwd(Some(&input), &mut paths);
    let mut want: Vec<PathBuf> = words.iter().take(3).map(PathBuf::from).collect();
    want.push(Path::new(cwd).join("brief.txt"));
    assert_eq!(paths, want);
}

/// macOS: a bare name that is a link into the data folder by its `/System/Volumes/Data/…`
/// spelling (a firmlink: the texts differ) is found by identity and refused.
#[cfg(target_os = "macos")]
#[test]
fn a_bare_name_into_the_data_folder_by_its_firmlinked_spelling_is_refused() {
    let dir = with_other("other-data-firmlink");
    let root = dir.path();
    let real = dunce::canonicalize(root.join("data")).unwrap();
    let data = Path::new("/System/Volumes/Data").join(real.strip_prefix("/").unwrap());
    if !data.join("skill-packs").exists() {
        return;
    }
    write::symlink(&data.join("skill-packs"), &root.join("matter/packs")).unwrap();
    assert_eq!(checked(root, &codex(root, "cat packs")), "other");
}

/// The launch's cache of the matters' folders is read again when the matter list changes: a
/// matter registered after a first request is checked on the next.
#[test]
fn the_cache_is_read_again_when_the_matter_list_changes() {
    let dir = with_other("other-cache");
    let root = dir.path();
    let command = "cp ../Other/brief.txt out.txt";
    let mut fence = codex_with(root, command, vec![root.join("outside")]);
    assert!(!reached(&fence));
    let cache = fence.other_matters.as_ref().unwrap().cache.clone();
    fence = codex_with(
        root,
        command,
        vec![root.join("outside"), root.join("Other")],
    );
    fence.other_matters.as_mut().unwrap().cache = cache;
    assert!(reached(&fence));
}

/// A PowerShell command is read by the one PowerShell reader (`pre_write_guard`'s
/// `powershell_words`): `'O''Brien'` is the folder `O'Brien`. Read so, a command reaching the
/// other matter `O'Brien` is refused, and one naming the matter's own `O'Brien` folder reaches
/// the card.
#[test]
fn a_powershell_word_with_a_doubled_apostrophe_is_read_as_powershell() {
    let dir = with_other("other-apostrophe");
    let root = dir.path();
    write::create_dir_all(&root.join("O'Brien")).unwrap();
    write::write_file(&root.join("O'Brien/brief.txt"), b"other").unwrap();
    write::create_dir_all(&root.join("matter/O'Brien")).unwrap();
    write::write_file(&root.join("matter/O'Brien/notes.txt"), b"own").unwrap();
    let answer = |command: &str| {
        let input = json!({"command": command, "cwd": root.join("matter").display().to_string()});
        let mut fence = check(root, &[], Some(&input));
        fence.other_matters = Some(OtherMatters {
            own: root.join("matter"),
            others: vec![root.join("O'Brien")],
            args: command_args(Some(&input), true),
            cache: super::Cache::default(),
        });
        checked(root, &fence)
    };
    assert_eq!(answer("Get-Content '../O''Brien/brief.txt'"), "other");
    assert_eq!(answer("Get-Content 'O''Brien/notes.txt'"), "card");
}
