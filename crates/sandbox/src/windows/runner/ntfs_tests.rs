//! The launch file on NTFS, and the message built from a real policy.

use std::collections::BTreeMap;
use std::ffi::OsString;

use super::paths::{CodexReuse, WinPath};
use super::*;
use crate::Policy;
use crate::windows::pool::PoolUser;
use crate::windows::test_acl::Place;
use crate::windows::write::make_agent_folder;

const POOL: &str = "S-1-5-21-1111111111-2222222222-3333333333-1005";

fn pool() -> PoolUser {
    PoolUser {
        name: "ClauseoSb01".to_owned(),
        sid: Sid::parse(POOL).unwrap(),
    }
}

fn folders(place: &Place, names: &[&str]) {
    for name in names {
        make_agent_folder(&place.at(name)).unwrap();
    }
}

fn spec(cwd: &Path) -> clauseo_process::LaunchSpec {
    let mut env = BTreeMap::new();
    env.insert(OsString::from("SystemRoot"), OsString::from(r"C:\Windows"));
    clauseo_process::LaunchSpec::new(
        PathBuf::from(r"C:\Windows\System32\whoami.exe"),
        vec![OsString::from("/all")],
        env,
        cwd.to_owned(),
    )
    .unwrap()
}

#[test]
fn a_written_launch_file_is_one_the_outer_runner_reads() {
    let place = Place::new("runner-launch-file");
    folders(&place, &["data", "matter", "state"]);
    let policy = Policy::builder(place.at("matter"), place.at("state"))
        .home(place.at("home"))
        .build()
        .unwrap();
    let message =
        LaunchMessage::from_policy(&policy, &pool(), &spec(&place.at("matter")), None).unwrap();
    let path = write_launch(&place.at("data"), &message).unwrap();
    let name = path.file_name().unwrap().to_str().unwrap();
    assert!(is_launch_name(name), "{name}");
    assert_eq!(path.parent().unwrap(), place.at(r"data\sandbox\run"));
    // The outer's read: the desktop user's alone, with its own protected DACL.
    let bytes = crate::windows::private_dir::read_unchanged(&path, LAUNCH_CAP)
        .unwrap()
        .expect("the outer runner reads the file");
    assert_eq!(LaunchMessage::parse(&bytes).unwrap(), message);
    assert!(crate::windows::test_acl::protected(&path));
    crate::windows::write::remove_owned_file(&path).unwrap();
}

#[test]
fn the_message_carries_the_spec_and_the_policy() {
    let place = Place::new("runner-from-policy");
    folders(&place, &["matter", "state", "notes", "tmp"]);
    let policy = Policy::builder(place.at("matter"), place.at("state"))
        .home(place.at("home"))
        .notes(place.at("notes"))
        .tmpdir(place.at("tmp"))
        .build()
        .unwrap();
    let bundle = place.at("roots.pem");
    let message =
        LaunchMessage::from_policy(&policy, &pool(), &spec(&place.at("matter")), Some(&bundle))
            .unwrap();
    assert_eq!(message.pool_name, "ClauseoSb01");
    assert_eq!(message.program, r"C:\Windows\System32\whoami.exe");
    assert_eq!(message.args, ["/all"]);
    assert_eq!(
        message.env,
        [("SystemRoot".to_owned(), r"C:\Windows".to_owned())]
    );
    assert_eq!(message.state, place.at("state").to_str().unwrap());
    assert_eq!(message.tmpdir.as_deref(), place.at("tmp").to_str());
    assert_eq!(message.ca_bundle.as_deref(), bundle.to_str());
    for granted in ["matter", "state", "notes", "tmp"] {
        let path = place.at(granted);
        assert!(
            message.granted.iter().any(|g| g == path.to_str().unwrap()),
            "{granted}: {:?}",
            message.granted
        );
    }
}

#[test]
fn m3_refuses_a_policy_in_the_lawyers_codex_folder() {
    let place = Place::new("runner-m3");
    folders(&place, &["matter", r"profile\.codex\state", "state"]);
    let profile = WinPath::parse(place.at("profile").to_str().unwrap()).unwrap();
    let clean = Policy::builder(place.at("matter"), place.at("state"))
        .home(place.at("home"))
        .build()
        .unwrap();
    assert_eq!(codex_reuse_in_policy(&clean, &profile), Ok(()));
    let reused = Policy::builder(place.at("matter"), place.at(r"profile\.codex\state"))
        .home(place.at("home"))
        .build()
        .unwrap();
    assert_eq!(
        codex_reuse_in_policy(&reused, &profile),
        Err(CodexReuse::Path("granted"))
    );
}
