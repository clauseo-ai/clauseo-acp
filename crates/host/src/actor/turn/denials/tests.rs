//! The tool-call source's jail: inside (nothing), outside, `..`, a link out of the matter, and
//! network paths (UNC, `/net`, a link to a share) found without touching them, their cards never
//! offered.

use std::path::{Path, PathBuf};

use clauseo_protocol::{DenialKind, MatterId};
use clauseo_sandbox::PathClass;

use super::network::tests::plain_net;
use super::offer::OfferRules;
use super::resolve::RealFs;
use super::resolve::tests::{Fake, FakeFs};
use super::{Unchecked, decide, outside_jail};
use crate::pipeline::tests::support::TempDir;

#[test]
fn a_tool_calls_path_is_a_denial_only_outside_the_jail() {
    let dir = TempDir::new("denial-jail");
    let root = dunce::canonicalize(dir.path()).unwrap();
    let (matter, data, elsewhere) = (root.join("Acme"), root.join("data"), root.join("Deals"));
    for folder in [&matter, &data, &elsewhere] {
        crate::write::create_dir_all(folder).unwrap();
    }
    let net = plain_net(cfg!(windows));
    let jail = |named: &[String]| outside_jail(&matter, &data, named, &RealFs, &net);
    let named = |paths: &[&Path]| -> Vec<String> {
        paths.iter().map(|p| p.display().to_string()).collect()
    };
    // Inside the matter, as a relative or an absolute path, and in the data folder: nothing.
    assert_eq!(jail(&["brief.txt".to_owned()]), None);
    assert_eq!(
        jail(&named(&[
            &matter.join("brief.txt"),
            &data.join("run").join("x")
        ])),
        None
    );
    // Outside: the first path that is.
    let outside = named(&[&matter.join("a.txt"), &elsewhere.join("spa.docx")]);
    let spa = Some((elsewhere.join("spa.docx"), PathClass::Outside));
    assert_eq!(jail(&outside), spa);
    // A `..` out of the matter is outside, not a link.
    assert_eq!(jail(&["../Deals/spa.docx".to_owned()]), spa);
    #[cfg(unix)]
    {
        crate::write::symlink(&elsewhere, &matter.join("link")).unwrap();
        assert_eq!(
            jail(&["link/spa.docx".to_owned()]),
            Some((elsewhere.join("spa.docx"), PathClass::LinkTarget))
        );
    }
}

fn rules() -> OfferRules {
    let matter = MatterId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0001,
    ))
    .unwrap();
    OfferRules {
        home: Some("/home/a".into()),
        data_dir: "/home/a/.clauseo".into(),
        matter,
        matter_root: "/home/a/Acme".into(),
        refused: Vec::new(),
        matters: Vec::new(),
        fold_case: false,
    }
}

#[test]
fn a_network_path_from_a_tool_call_is_never_touched_nor_offered() {
    let fs = FakeFs::with(&[
        ("/home/a/Acme/brief.txt", Fake::File),
        ("/home/a/Acme/share", Fake::Link(r"\\server\share".into())),
    ]);
    let net = plain_net(false);
    let (folder, data) = (Path::new("/home/a/Acme"), Path::new("/home/a/.clauseo"));
    let unc = r"\\server\share\brief.docx".to_owned();
    let got = outside_jail(folder, data, std::slice::from_ref(&unc), &fs, &net);
    assert_eq!(got, Some((PathBuf::from(&unc), PathClass::Outside)));
    let slash_net = "/net/server/x.docx".to_owned();
    let got = outside_jail(folder, data, std::slice::from_ref(&slash_net), &fs, &net);
    assert_eq!(got, Some((PathBuf::from(&slash_net), PathClass::Outside)));
    assert!(fs.touched().is_empty(), "{:?}", fs.touched());

    // The card: the path as written, not offered, and flagged network for the log.
    let (offer, unchecked) = decide(&rules(), Path::new(&unc), PathClass::Outside, &fs, &net);
    assert_eq!(unchecked, Some(Unchecked::Network));
    assert!(!offer.offered);
    assert_eq!(
        offer.kind,
        DenialKind::File {
            path: unc,
            other_matter: None
        }
    );
    assert!(fs.touched().is_empty(), "{:?}", fs.touched());

    // Through a link in the matter: stopped at the link, the share never looked at.
    let got = outside_jail(folder, data, &["share/x.docx".to_owned()], &fs, &net);
    assert_eq!(
        got,
        Some((
            PathBuf::from("/home/a/Acme/share/x.docx"),
            PathClass::LinkTarget
        ))
    );
    let touched = fs.touched();
    assert!(
        touched
            .iter()
            .all(|p| !p.starts_with("/home/a/Acme/share/x.docx")),
        "{touched:?}"
    );
}

#[test]
fn a_matter_on_a_share_keeps_its_own_paths_inside_untouched() {
    let fs = FakeFs::default();
    let net = plain_net(false);
    let folder = Path::new(r"\\server\share\Acme");
    let named = [r"\\server\share\Acme\brief.txt".to_owned()];
    assert_eq!(
        outside_jail(folder, Path::new("/data"), &named, &fs, &net),
        None
    );
    assert_eq!(fs.touched(), Vec::<PathBuf>::new());
}

#[test]
fn an_unresolved_path_is_refused_never_offered() {
    let fs = FakeFs::with(&[
        ("/srv/a", Fake::Link("/srv/b".into())),
        ("/srv/b", Fake::Link("/srv/a".into())),
        ("/srv/broken", Fake::BrokenLink),
        ("/srv/locked", Fake::Failing),
    ]);
    let net = plain_net(false);
    for path in ["/srv/a/x.docx", "/srv/broken/x.docx", "/srv/locked/x.docx"] {
        let (offer, unchecked) = decide(&rules(), Path::new(path), PathClass::Outside, &fs, &net);
        assert_eq!(unchecked, Some(Unchecked::Unresolved), "{path}");
        assert!(!offer.offered, "{path}");
        assert_eq!(
            offer.kind,
            DenialKind::File {
                path: path.to_owned(),
                other_matter: None
            }
        );
        // The tool-call source keeps it too: not known to be inside the jail.
        let named = [path.to_owned()];
        let jail = outside_jail(
            Path::new("/home/a/Acme"),
            Path::new("/d"),
            &named,
            &fs,
            &net,
        );
        assert_eq!(
            jail,
            Some((PathBuf::from(path), PathClass::Outside)),
            "{path}"
        );
    }
}
