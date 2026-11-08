//! The offer rules as a table, then `canonical` on a real folder: links, `..`, a missing tail.

use std::path::Path;

use clauseo_protocol::{DenialKind, MatterId, OtherMatter};
use clauseo_sandbox::{HostRefusal, PathClass};
use uuid::Uuid;

use super::{KnownMatter, Offer, OfferRules, Refused, fold_name, offer};

fn matter(n: u128) -> MatterId {
    MatterId::new(Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0000 | n,
    ))
    .unwrap()
}

/// `/home/a`, its data folder under `.local`, this matter `Acme` and another, `Smith`.
fn rules(fold_case: bool) -> OfferRules {
    OfferRules {
        home: Some("/home/a".into()),
        data_dir: "/home/a/.local/share/clauseo".into(),
        matter: matter(1),
        matter_root: "/home/a/Matters/Acme".into(),
        refused: vec![
            "/home/a/.codex".into(),
            "/usr".into(),
            "/home/a/Library".into(),
        ],
        matters: vec![
            KnownMatter {
                id: matter(1),
                name: "Acme".into(),
                root: "/home/a/Matters/Acme".into(),
            },
            KnownMatter {
                id: matter(2),
                name: "Smith".into(),
                root: "/home/a/Matters/Smith".into(),
            },
        ],
        fold_case,
    }
}

fn folder(path: &str) -> DenialKind {
    DenialKind::Folder {
        path: path.into(),
        other_matter: None,
    }
}

fn file(path: &str) -> DenialKind {
    DenialKind::File {
        path: path.into(),
        other_matter: None,
    }
}

fn smith() -> OtherMatter {
    OtherMatter {
        matter: matter(2),
        name: "Smith".into(),
    }
}

fn path_offer(rules: &OfferRules, path: &str, is_file: bool, class: PathClass) -> Offer {
    offer(
        rules,
        Refused::Path {
            path: Path::new(path),
            is_file,
            class,
        },
    )
}

#[test]
fn paths_give_the_cards_the_rules_allow() {
    use PathClass::{Hidden, LinkTarget, Outside, ReadOnly};
    let other_file = |p: &str| DenialKind::File {
        path: p.into(),
        other_matter: Some(smith()),
    };
    let other_folder = |p: &str| DenialKind::Folder {
        path: p.into(),
        other_matter: Some(smith()),
    };
    #[rustfmt::skip]
    let table: Vec<(&str, bool, PathClass, DenialKind, bool)> = vec![
        // A file outside every matter: its folder.
        ("/home/a/Deals/spa.docx", true, Outside, folder("/home/a/Deals"), true),
        ("/home/a/Deals", false, Outside, folder("/home/a/Deals"), true),
        // Directly in home: the one file, never home.
        ("/home/a/notes.docx", true, Outside, file("/home/a/notes.docx"), true),
        ("/home/a", false, Outside, folder("/home/a"), false),
        ("/home", false, Outside, folder("/home"), false),
        ("/", false, Outside, folder("/"), false),
        // Keys, dot files and credential stores: not even one file.
        ("/home/a/.ssh/id_ed25519", true, Outside, folder("/home/a/.ssh"), false),
        ("/home/a/.netrc", true, Outside, folder("/home/a"), false),
        ("/home/a/Keychains/login.db", true, Outside, folder("/home/a/Keychains"), false),
        // Clauseo's data folder and the refused trees.
        ("/home/a/.local/share/clauseo/x.json", true, Hidden, folder("/home/a/.local/share/clauseo"), false),
        ("/home/a/.local/share/clauseo/x.json", true, Outside, folder("/home/a/.local/share/clauseo"), false),
        ("/usr/share/dict/words", true, Outside, folder("/usr/share/dict"), false),
        ("/home/a/Library/Mail/x", true, Outside, folder("/home/a/Library/Mail"), false),
        ("/home/a/.codex/auth.json", true, Outside, folder("/home/a/.codex"), false),
        // A .git anywhere, and its folder.
        ("/home/a/proj/.git/config", true, Outside, folder("/home/a/proj/.git"), false),
        ("/home/a/proj/.git", false, Outside, folder("/home/a/proj/.git"), false),
        // This matter, the folder holding the matters, and the classes never offered.
        ("/home/a/Matters/Acme/sub", false, Outside, folder("/home/a/Matters/Acme/sub"), false),
        ("/home/a/Matters/x.docx", true, Outside, file("/home/a/Matters/x.docx"), true),
        ("/home/a/Matters", false, Outside, folder("/home/a/Matters"), false),
        ("/home/a/Deals/spa.docx", true, ReadOnly, folder("/home/a/Deals"), false),
        ("/home/a/Deals/spa.docx", true, LinkTarget, folder("/home/a/Deals"), false),
        ("/home/a/Deals/spa.docx", true, Hidden, folder("/home/a/Deals"), false),
        ("/home/a/notes.docx", true, Hidden, folder("/home/a"), false),
        // Another matter: the asked path alone, never its root or its dot folders.
        ("/home/a/Matters/Smith/brief.docx", true, Hidden, other_file("/home/a/Matters/Smith/brief.docx"), true),
        ("/home/a/Matters/Smith/brief.docx", true, Outside, other_file("/home/a/Matters/Smith/brief.docx"), true),
        ("/home/a/Matters/Smith/Pleadings", false, Hidden, other_folder("/home/a/Matters/Smith/Pleadings"), true),
        ("/home/a/Matters/Smith", false, Hidden, other_folder("/home/a/Matters/Smith"), false),
        ("/home/a/Matters/Smith/.claude/settings.json", true, Hidden, other_file("/home/a/Matters/Smith/.claude/settings.json"), false),
        ("/home/a/Matters/Smith/.clauseo/notes.md", true, Hidden, other_file("/home/a/Matters/Smith/.clauseo/notes.md"), false),
        ("/home/a/Matters/Smith/.codex", false, Hidden, other_folder("/home/a/Matters/Smith/.codex"), false),
        ("/home/a/Matters/Smith/.agents/x", true, Hidden, other_file("/home/a/Matters/Smith/.agents/x"), false),
        ("/home/a/Matters/Smith/.git/HEAD", true, Hidden, other_file("/home/a/Matters/Smith/.git/HEAD"), false),
        ("/home/a/Matters/Smith/brief.docx", true, LinkTarget, other_file("/home/a/Matters/Smith/brief.docx"), false),
        ("/home/a/Matters/Smith/brief.docx", true, ReadOnly, other_file("/home/a/Matters/Smith/brief.docx"), false),
        // A name that only starts like a matter's is not in it.
        ("/home/a/Matters/Smithson/a.docx", true, Outside, folder("/home/a/Matters/Smithson"), true),
    ];
    let rules = rules(false);
    for (path, is_file, class, kind, offered) in table {
        let got = path_offer(&rules, path, is_file, class);
        assert_eq!(got, Offer { kind, offered }, "{path} ({class:?})");
    }
}

#[test]
fn refused_names_compare_folded_on_every_system() {
    for (name, folded) in [
        (".clau\u{17F}eo", ".clauseo"),
        (".CLAUDE", ".claude"),
        (".agent\u{17F}", ".agents"),
        (".Git", ".git"),
        ("\u{212A}eychains", "keychains"),
        ("\u{212A}EYRINGS", "keyrings"),
        ("Credential\u{17F}", "credentials"),
    ] {
        assert_eq!(fold_name(name), folded, "{name}");
    }
    // Never offered, with folding off (a Linux volume) as much as on.
    for fold_case in [false, true] {
        let rules = rules(fold_case);
        for path in [
            "/home/a/Deals/.clau\u{17F}eo/notes.md",
            "/home/a/Deals/.CLAUDE/settings.json",
            "/home/a/Deals/.agent\u{17F}/x",
            "/home/a/proj/.Git/config",
            "/home/a/\u{212A}eychains/login.db",
            "/home/a/.local/\u{212A}EYRINGS/x",
            "/home/a/AppData/Microsoft/Credential\u{17F}/x",
            "/home/a/Matters/Smith/.clau\u{17F}eo/notes.md",
        ] {
            let got = path_offer(&rules, path, true, PathClass::Outside);
            assert!(!got.offered, "{path} (fold_case {fold_case}): {got:?}");
            let hidden = path_offer(&rules, path, true, PathClass::Hidden);
            assert!(!hidden.offered, "{path} hidden (fold_case {fold_case})");
        }
    }
    // Where names fold, `ſmith` is Smith's folder: the asked file alone, as another matter's.
    let got = path_offer(
        &rules(true),
        "/home/a/Matters/\u{17F}mith/brief.docx",
        true,
        PathClass::Hidden,
    );
    assert_eq!(
        got.kind,
        DenialKind::File {
            path: "/home/a/Matters/\u{17F}mith/brief.docx".into(),
            other_matter: Some(smith())
        }
    );
}

#[test]
fn names_fold_case_where_the_volume_does() {
    let folding = rules(true);
    let got = path_offer(
        &folding,
        "/HOME/A/matters/SMITH/brief.docx",
        true,
        PathClass::Hidden,
    );
    assert_eq!(
        got.kind,
        DenialKind::File {
            path: "/HOME/A/matters/SMITH/brief.docx".into(),
            other_matter: Some(smith())
        }
    );
    assert!(got.offered);
    let home = path_offer(&folding, "/Home/A", false, PathClass::Outside);
    assert!(!home.offered, "home in another case is still home");
    let usr = path_offer(&folding, "/USR/lib/x", true, PathClass::Outside);
    assert!(
        !usr.offered,
        "a refused tree in another case is still refused"
    );
    let keys = path_offer(&folding, "/home/a/KEYCHAINS/x", true, PathClass::Outside);
    assert!(!keys.offered);
    // Without folding, the other case is another folder.
    let exact = path_offer(&rules(false), "/Home/A", false, PathClass::Outside);
    assert!(exact.offered);
}

#[cfg(windows)]
#[test]
fn windows_drive_paths_fold_case() {
    let rules = OfferRules {
        home: Some(r"C:\Users\a".into()),
        data_dir: r"C:\Users\a\AppData\Roaming\Clauseo".into(),
        matter: matter(1),
        matter_root: r"C:\Users\a\Matters\Acme".into(),
        refused: vec![r"C:\Users\a\AppData".into()],
        matters: vec![KnownMatter {
            id: matter(2),
            name: "Smith".into(),
            root: r"C:\Users\a\Matters\Smith".into(),
        }],
        fold_case: true,
    };
    assert!(!path_offer(&rules, r"C:\", false, PathClass::Outside).offered);
    assert!(!path_offer(&rules, r"c:\users\A", false, PathClass::Outside).offered);
    let one = path_offer(&rules, r"C:\Users\a\notes.docx", true, PathClass::Outside);
    assert_eq!(one.kind, file(r"C:\Users\a\notes.docx"));
    assert!(one.offered);
    let other = path_offer(
        &rules,
        r"c:\users\a\matters\smith\x.docx",
        true,
        PathClass::Hidden,
    );
    assert!(other.offered);
    assert!(matches!(
        other.kind,
        DenialKind::File {
            other_matter: Some(_),
            ..
        }
    ));
    let roaming = r"C:\Users\a\appdata\Roaming\Microsoft\Credentials\x";
    assert!(!path_offer(&rules, roaming, true, PathClass::Outside).offered);
}

#[test]
fn sites_are_offered_only_when_not_on_the_list() {
    let rules = rules(false);
    let site = |why| {
        offer(
            &rules,
            Refused::Site {
                host: "example.com",
                shared_hosting: true,
                why,
            },
        )
    };
    let allowed = site(HostRefusal::NotAllowed);
    assert_eq!(
        allowed.kind,
        DenialKind::Site {
            host: "example.com".into(),
            shared_hosting: true
        }
    );
    assert!(allowed.offered);
    for why in [
        HostRefusal::PrivateAddress,
        HostRefusal::Port,
        HostRefusal::Scheme,
    ] {
        assert!(!site(why).offered, "{why:?}");
    }
    let network = offer(
        &rules,
        Refused::Unchecked {
            path: r"\\server\share\x.docx",
        },
    );
    assert_eq!(
        network,
        Offer {
            kind: file(r"\\server\share\x.docx"),
            offered: false
        }
    );
    let auth = offer(
        &rules,
        Refused::UpstreamAuth {
            upstream: "proxy.firm.example",
        },
    );
    assert!(!auth.offered);
}
