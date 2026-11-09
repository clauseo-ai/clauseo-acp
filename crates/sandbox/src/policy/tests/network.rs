//! Every settings path is classified, name by name and link by link, before `build` touches
//! it: a link above a folder to a share is never followed.

use super::*;
use crate::links::network::Mounts;

/// A network mount nobody can reach: following a link into it would fail differently.
fn share() -> Mounts {
    Mounts::new(vec![(PathBuf::from("/Volumes/share"), true)], b"Z")
}

/// A matter written through a link above it to a share (`Shares` links to the share) refuses
/// with the network message, before `root_folder` resolves it (that would fail with "cannot be
/// resolved", since the share isn't there). So does one whose `..` leads onto the share.
#[cfg(unix)]
#[test]
fn a_matter_through_a_link_to_a_share_refuses_before_it_is_resolved() {
    let w = World::new("net-matter-link");
    w.dir.symlink("Shares", Path::new("/Volumes/share"));
    let built = Policy::builder(w.at("Shares/m"), w.at("state"))
        .home(w.at("home"))
        .mounts(share())
        .build();
    assert!(matches!(built, Err(Error::MatterOnNetwork)), "{built:?}");
    let dots = w.at("matter/../Shares/m");
    let built = Policy::builder(dots, w.at("state"))
        .home(w.at("home"))
        .mounts(share())
        .build();
    assert!(matches!(built, Err(Error::MatterOnNetwork)), "{built:?}");
}

/// A matter on a mapped network drive or a server path refuses through `build`.
#[test]
fn a_matter_on_a_network_drive_refuses_through_build() {
    for matter in [r"Z:\matters\m", r"\\server\share\m", "/Volumes/share/m"] {
        let built = Policy::builder(matter, "/state").mounts(share()).build();
        assert!(
            matches!(built, Err(Error::MatterOnNetwork)),
            "{matter}: {built:?}"
        );
    }
}

/// Every other path the agent is given refuses when it reaches a share, through a link or as
/// written, before anything resolves it.
#[cfg(unix)]
#[test]
fn a_settings_path_on_a_share_refuses() {
    let w = World::new("net-settings");
    w.dir.symlink("net", Path::new("/Volumes/share"));
    let net = |name: &str| w.at("net").join(name);
    let cases: [(&str, PolicyBuilder); 11] = [
        (
            "state folder",
            Policy::builder(w.at("matter"), net("state")),
        ),
        ("home folder", w.builder().home(net("home"))),
        ("temp folder", w.builder().tmpdir(net("run/s/tmp"))),
        ("notes folder", w.builder().notes(net("notes"))),
        (
            "credential link",
            w.builder().credential_link(net("credentials.json")),
        ),
        (
            "Claude store folder",
            w.builder().claude_store(net("claude")),
        ),
        ("runtime path", w.builder().runtime(net("node"))),
        ("read path", w.builder().read(net("ca.pem"))),
        ("allowed folder", w.builder().allowed_folder(net("docs"))),
        (
            "read-only path in the state folder",
            w.builder().read_only_in_state(net("state/keep")),
        ),
        ("over-cap file", w.builder().over_cap([net("big.bin")])),
    ];
    for (what, builder) in cases {
        let built = builder.mounts(share()).build();
        let rule = format!("the {what} is on a network drive");
        assert!(
            matches!(&built, Err(Error::Policy(m)) if m.contains(&rule)),
            "{what}: {built:?}"
        );
    }
}

/// Another registered matter on a share is kept as written, never resolved: the launch goes on
/// and still hides it.
#[cfg(unix)]
#[test]
fn a_hidden_path_on_a_share_is_kept_as_written() {
    let w = World::new("net-hidden");
    let other = PathBuf::from("/Volumes/share/other-matter");
    let policy = w
        .builder()
        .hidden(other.clone())
        .mounts(share())
        .build()
        .unwrap();
    assert!(policy.hidden().contains(&other), "{:?}", policy.hidden());
}

/// A link finding the host passes is classified before it is touched: a target through a link
/// to a share stays as written (unresolved), and a finding whose folder is there refuses.
#[cfg(unix)]
#[test]
fn a_host_link_finding_on_a_share_is_never_resolved() {
    let w = World::new("net-findings");
    w.dir.symlink("net", Path::new("/Volumes/share"));
    let finding = |path: PathBuf, target: PathBuf| LinkFinding {
        path,
        kind: crate::LinkKind::Symlink,
        target: Some(target),
    };
    let target = w.at("net/x");
    let policy = w
        .builder()
        .scan(crate::MatterScan::default())
        .links([finding(w.at("matter/l"), target.clone())])
        .mounts(share())
        .build()
        .unwrap();
    let got: Vec<&Path> = policy
        .links()
        .iter()
        .filter_map(|f| f.target.as_deref())
        .collect();
    assert_eq!(got, [target.as_path()]);

    let built = w
        .builder()
        .scan(crate::MatterScan::default())
        .links([finding(w.at("net/l"), w.at("elsewhere"))])
        .mounts(share())
        .build();
    refused(
        built,
        "a link finding must be inside the matter",
        &w.at("net/l"),
    );
}
