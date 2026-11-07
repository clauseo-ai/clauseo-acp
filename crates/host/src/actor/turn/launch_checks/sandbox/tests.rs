//! The launch's jail, its pure parts: the set-aside names made relative, the notice, the
//! failure's message without a path, and B2's policy.

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, MatterId, ThreadId};
use clauseo_sandbox::SetAside;

use super::{NAMED, launch_policy, notice, refused, relative_names};
use crate::actor::turn::launch_checks::RunFolders;
use crate::config::SessionKey;
use crate::pipeline::tests::support::id;
use crate::write;

fn moved(from: &Path) -> SetAside {
    SetAside {
        from: from.to_owned(),
        to: PathBuf::from("/data/sandbox/set-aside/m/1").join(from.file_name().unwrap()),
    }
}

#[test]
fn set_aside_names_are_relative_to_the_matter_and_never_absolute() {
    let canonical = Path::new("/private/var/m");
    let stored = Path::new("/var/m");
    let set_aside = [
        moved(&canonical.join(".git")),
        moved(&stored.join("sub").join("AGENTS.md")),
        moved(Path::new("/elsewhere/deep/CLAUDE.md")),
        moved(Path::new("sub/.claude")),
    ];
    assert_eq!(
        relative_names(&set_aside, &[canonical, stored]),
        [".git", "sub/AGENTS.md", "CLAUDE.md", "sub/.claude"]
    );
}

#[test]
fn one_notice_names_the_first_ten_and_counts_the_rest() {
    let two = notice(&[".git".to_owned(), "sub/AGENTS.md".to_owned()]);
    assert!(two.text.contains(".git, sub/AGENTS.md"), "{}", two.text);
    assert!(two.text.contains("2 protected items"), "{}", two.text);
    assert_eq!(two.kind, None);
    let many: Vec<String> = (0..NAMED + 2).map(|n| format!("f{n}/.git")).collect();
    let text = notice(&many).text;
    assert!(
        text.contains("f9/.git") && !text.contains("f10/.git"),
        "{text}"
    );
    assert!(text.ends_with("and 2 more."), "{text}");
}

#[test]
fn a_failure_names_its_kind_and_never_its_path() {
    let error = refused(&clauseo_sandbox::Error::Policy(
        "the matter /Users/a/Matters/Smith holds the home folder".to_owned(),
    ));
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
    let message = error.to_string();
    assert!(
        message.contains("rules for this matter were refused"),
        "{message}"
    );
    assert!(!message.contains("/Users"), "{message}");
    let link = refused(&clauseo_sandbox::Error::ProtectedLink {
        at: PathBuf::from("sub/AGENTS.md"),
    });
    assert_eq!(link.code(), ErrorCode::MatterProtectedLink);
    assert!(!link.to_string().contains("AGENTS.md"), "{link}");
}

#[test]
fn the_policy_grants_the_matter_the_session_tmp_and_the_install() {
    let root = std::env::temp_dir().join(format!("clauseo-b2-policy-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    let (data, matter, install) = (root.join("data"), root.join("m"), root.join("agent"));
    for folder in [&matter, &install] {
        write::create_dir_all(folder).unwrap();
    }
    let run = RunFolders::new(
        &data,
        SessionKey {
            matter: id::<MatterId>(1),
            thread: id::<ThreadId>(2),
            launch: 3,
        },
    );
    write::create_dir_all(&run.work).unwrap();
    let policy = launch_policy(&matter, &run, &install).unwrap();
    let real = |path: &Path| dunce::canonicalize(path).unwrap();
    assert_eq!(policy.matter(), real(&matter));
    assert_eq!(policy.state(), real(&run.work));
    assert_eq!(
        policy.session_tmpdir(),
        Some(real(&run.root).join("tmp").as_path())
    );
    assert_eq!(policy.runtime(), [real(&install)]);
    assert!(policy.read().is_empty() && policy.allowed_folders().is_empty());
    write::remove_dir_all(&root).unwrap();
}
