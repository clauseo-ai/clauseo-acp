//! Unicode normalisation in path comparisons: on macOS an NFC, an NFD and a raw name open one
//! folder, so every form compares equal there; elsewhere they are two names. Hindi content is a
//! hard requirement, so the names here are Devanagari with nukta letters.

use unicode_normalization::UnicodeNormalization;

use super::*;
#[cfg(target_os = "macos")]
use crate::LinkKind;
use crate::policy::names::{path_key, same_name};

/// `क़ानून ऩ` with क़ as U+0958, a composition exclusion: NFC and NFD both decompose it.
const RAW: &str = "\u{0958}\u{093E}\u{0928}\u{0942}\u{0928} \u{0929}";
/// [`RAW`] in NFC: क़ decomposed, ऩ (U+0929) composed.
const NFC: &str = "\u{0915}\u{093C}\u{093E}\u{0928}\u{0942}\u{0928} \u{0929}";
/// [`RAW`] in NFD: both nukta letters decomposed.
const NFD: &str = "\u{0915}\u{093C}\u{093E}\u{0928}\u{0942}\u{0928} \u{0928}\u{093C}";
/// `ग़ुप्त ऩऱ`, a hidden folder's name, in NFC.
const HIDDEN_NFC: &str = "\u{0917}\u{093C}\u{0941}\u{092A}\u{094D}\u{0924} \u{0929}\u{0931}";
/// [`HIDDEN_NFC`] in NFD.
const HIDDEN_NFD: &str =
    "\u{0917}\u{093C}\u{0941}\u{092A}\u{094D}\u{0924} \u{0928}\u{093C}\u{0930}\u{093C}";

#[test]
fn the_test_names_are_three_different_forms() {
    assert_eq!(RAW.nfc().collect::<String>(), NFC);
    assert_eq!(RAW.nfd().collect::<String>(), NFD);
    assert_eq!(HIDDEN_NFD.nfc().collect::<String>(), HIDDEN_NFC);
    assert_eq!(HIDDEN_NFC.nfd().collect::<String>(), HIDDEN_NFD);
    assert!(RAW != NFC && RAW != NFD && NFC != NFD);
    assert_ne!(HIDDEN_NFC, HIDDEN_NFD);
}

#[cfg(target_os = "macos")]
#[test]
fn every_form_of_a_name_is_the_same_on_macos() {
    for a in [RAW, NFC, NFD] {
        for b in [RAW, NFC, NFD] {
            assert!(same(Path::new(a), Path::new(b)), "{a:?} and {b:?}");
            assert!(under(Path::new(a), Path::new(b)), "{a:?} under {b:?}");
            let long = PathBuf::from(format!("/Users/x/{a}/drafts/{a}.docx"));
            assert!(under(&long, &PathBuf::from(format!("/Users/x/{b}"))));
            assert!(same(
                &long,
                &PathBuf::from(format!("/Users/x/{b}/drafts/{b}.docx"))
            ));
            assert!(!same(&long, &PathBuf::from(format!("/Users/x/{b}"))));
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn ascii_case_still_folds_on_macos() {
    assert!(under(Path::new("/A/Data/x"), Path::new("/a/DATA")));
    assert!(same(Path::new("/m/.GIT"), Path::new("/m/.git")));
    // Case folds beside a Devanagari name too.
    let upper = PathBuf::from(format!("/M/{NFC}/CLAUDE.md"));
    assert!(same(&upper, &PathBuf::from(format!("/m/{NFD}/claude.md"))));
}

#[test]
fn different_letters_are_never_the_same() {
    // क against क़: the nukta makes another letter.
    assert!(!same(Path::new("/m/\u{0915}"), Path::new("/m/\u{0958}")));
    assert!(!under(Path::new("/m/\u{0958}/x"), Path::new("/m/\u{0915}")));
    // One extra nukta is another name, in every form.
    for form in [RAW, NFC, NFD] {
        let extra = format!("/m/{form}\u{093C}");
        let plain = format!("/m/{form}");
        assert!(!same(Path::new(&extra), Path::new(&plain)), "{extra:?}");
        assert!(!under(Path::new(&extra), Path::new(&plain)), "{extra:?}");
        assert!(!under(Path::new(&plain), Path::new(&extra)), "{extra:?}");
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn forms_differ_off_macos() {
    // NTFS and Linux file systems keep NFC and NFD names as two files.
    assert!(!same(Path::new(NFC), Path::new(NFD)));
    assert!(!under(
        &PathBuf::from(format!("/m/{NFC}/x")),
        &PathBuf::from(format!("/m/{NFD}"))
    ));
    assert!(!same_name("\u{00C9}".as_ref(), "\u{00E9}".as_ref()));
    assert_eq!(path_key(Path::new(NFD)), Path::new(NFD));
}

/// A world whose matter is named [`NFD`] and made in that form, beside the usual folders.
#[cfg(target_os = "macos")]
fn devanagari_world(test: &str) -> World {
    let w = World::new(test);
    w.dir.dir(NFD);
    w
}

#[cfg(target_os = "macos")]
fn builder_for(w: &World, matter: &str) -> PolicyBuilder {
    Policy::builder(w.at(matter), w.at("state")).home(w.at("home"))
}

#[cfg(target_os = "macos")]
#[test]
fn a_matter_made_in_nfd_builds_from_any_form_in_its_stored_form() {
    let w = devanagari_world("nfc-matter");
    let stored = canon(w.dir.path()).join(NFD);
    for given in [NFC, RAW, NFD] {
        let policy = builder_for(&w, given).build().unwrap();
        assert_eq!(policy.matter().as_os_str(), stored.as_os_str(), "{given:?}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn classify_finds_the_matter_in_another_form() {
    let w = devanagari_world("nfc-classify");
    let policy = builder_for(&w, NFC).build().unwrap();
    let root = canon(w.dir.path());
    for form in [NFC, RAW] {
        let matter = root.join(form);
        for rel in ["CLAUDE.md", "sub/claude.md", ".clauseo/matter.json"] {
            assert_eq!(
                policy.classify(&matter.join(rel), Access::Write),
                PathClass::ReadOnly,
                "{form:?}/{rel}"
            );
        }
        assert_eq!(
            policy.classify(&matter.join("brief.docx"), Access::Write),
            PathClass::Outside
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_hidden_devanagari_folder_is_hidden_in_any_form() {
    let w = devanagari_world("nfc-hidden");
    w.dir.dir(HIDDEN_NFC);
    let policy = builder_for(&w, NFD)
        .hidden(w.at(HIDDEN_NFC))
        .build()
        .unwrap();
    let root = canon(w.dir.path());
    assert_eq!(policy.hidden(), [root.join(HIDDEN_NFC)]);
    for form in [HIDDEN_NFD, HIDDEN_NFC] {
        assert_eq!(
            policy.classify(&root.join(form).join("x.pdf"), Access::Read),
            PathClass::Hidden,
            "{form:?}"
        );
    }
    let rule = "an allowed folder may not equal, hold or sit inside a hidden path";
    let other_form = w.at(HIDDEN_NFD);
    match policy.check_allowed_folder(&other_form) {
        Err(Error::Policy(message)) => assert!(message.contains(rule), "{message}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    // The rule itself, on a path not canonicalised first.
    let uncanonical = root.join(HIDDEN_NFD).join("inner");
    assert!(build::check_allowed(&uncanonical, policy.matter(), None, policy.hidden()).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn a_link_to_the_matter_in_another_form_leads_nowhere_outside() {
    let w = devanagari_world("nfc-link");
    w.dir.file(&format!("{NFD}/doc.txt"), "d");
    let target = canon(w.dir.path()).join(NFC).join("doc.txt");
    w.dir.symlink(&format!("{NFD}/link"), &target);
    let policy = builder_for(&w, NFC).build().unwrap();
    assert!(policy.links().is_empty(), "{:?}", policy.links());
}

/// Pairs of names to check against this Mac's volume: Devanagari forms and nukta letters,
/// accented and Greek capitals, the full folds (ß, ﬁ, İ), compatibility-free singletons (Kelvin,
/// Angstrom), long s, fullwidth letters and Hangul.
#[cfg(target_os = "macos")]
const PAIRS: [(&str, &str); 16] = [
    (RAW, NFC),
    (RAW, NFD),
    (NFC, NFD),
    ("\u{0915}", "\u{0958}"),
    ("\u{00C9}crits", "\u{00E9}crits"),
    ("\u{03A3}", "\u{03C3}"),
    ("\u{03A3}", "\u{03C2}"),
    ("\u{03C3}", "\u{03C2}"),
    ("stra\u{00DF}e", "strasse"),
    ("\u{FB01}le", "file"),
    ("\u{0130}", "i\u{0307}"),
    ("K", "\u{212A}"),
    ("\u{00C5}", "\u{212B}"),
    ("\u{017F}", "s"),
    ("\u{FF21}", "A"),
    ("\u{D55C}", "\u{1112}\u{1161}\u{11AB}"),
];

#[cfg(target_os = "macos")]
#[test]
fn names_match_exactly_when_this_volume_opens_one_folder_for_both() {
    use std::os::unix::fs::MetadataExt;
    let dir = crate::test_support::TempDir::new("nfc-apfs-pairs");
    for (i, (a, b)) in PAIRS.into_iter().enumerate() {
        let made = dir.dir(&format!("p{i}/{a}"));
        let inode = std::fs::metadata(&made).unwrap().ino();
        let opened = std::fs::metadata(dir.path().join(format!("p{i}/{b}")));
        let one_folder = opened.is_ok_and(|meta| meta.ino() == inode);
        assert_eq!(
            same_name(a.as_ref(), b.as_ref()),
            one_folder,
            "{a:?} and {b:?}: the volume opens one folder: {one_folder}"
        );
        assert_eq!(
            same_name(b.as_ref(), a.as_ref()),
            one_folder,
            "{b:?} and {a:?}"
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn an_accented_capital_hides_its_small_form() {
    let w = World::new("nfc-accent");
    // Not made: a hidden path keeps the names that do not exist as written.
    let policy = w.builder().hidden(w.at("\u{00C9}CRITS")).build().unwrap();
    let root = canon(w.dir.path());
    assert_eq!(
        policy.classify(&root.join("\u{00E9}crits/x.pdf"), Access::Read),
        PathClass::Hidden
    );
    let small = root.join("e\u{0301}crits");
    assert!(build::check_allowed(&small, policy.matter(), None, policy.hidden()).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn a_link_given_in_another_form_is_one_finding() {
    let w = World::new("nfc-link-twice");
    let secret = w.dir.file("other/secret.txt", "s");
    w.dir.symlink(&format!("matter/{NFD}"), &secret);
    let policy = w
        .builder()
        .links([LinkFinding {
            path: w.at(&format!("matter/{NFC}")),
            kind: LinkKind::Symlink,
            target: Some(secret),
        }])
        .build()
        .unwrap();
    assert_eq!(policy.links().len(), 1, "{:?}", policy.links());
    // The walk's form, the stored one, is kept.
    assert_eq!(policy.links()[0].path, canon(&w.at("matter")).join(NFD));
}

#[cfg(target_os = "macos")]
#[test]
fn an_over_cap_file_given_in_two_forms_is_one_rule() {
    let w = World::new("nfc-over-cap");
    w.dir.file(&format!("matter/{NFC}.pdf"), "large");
    let policy = w
        .builder()
        .over_cap([
            w.at(&format!("matter/{NFC}.pdf")),
            w.at(&format!("matter/{NFD}.pdf")),
        ])
        .build()
        .unwrap();
    let literals = policy
        .read_only_in_matter()
        .iter()
        .filter(|rule| matches!(rule, PathRule::Literal(_)))
        .count();
    assert_eq!(literals, 1, "{:?}", policy.read_only_in_matter());
}

/// Thousands of Devanagari-named findings, symbolic links out of the matter and hard links to a
/// nested `CLAUDE.md`, with the host passing the links again in another form: each finding is
/// kept once (review: the dedup was once quadratic, and kept both forms).
#[cfg(target_os = "macos")]
#[test]
fn thousands_of_devanagari_findings_are_each_kept_once() {
    const N: usize = 3000;
    let w = World::new("nfc-many");
    let secret = w.dir.file("other/secret.txt", "s");
    let original = w.dir.file("matter/sub/CLAUDE.md", "c");
    w.dir.dir("matter/a");
    w.dir.dir("matter/b");
    let mut again = Vec::with_capacity(N);
    for i in 0..N {
        w.dir.symlink(&format!("matter/a/{NFC} {i}"), &secret);
        w.dir
            .hard_link(&format!("matter/b/{HIDDEN_NFC} {i}"), &original);
        again.push(LinkFinding {
            path: w.at(&format!("matter/a/{NFD} {i}")),
            kind: LinkKind::Symlink,
            target: Some(secret.clone()),
        });
    }
    let policy = w.builder().links(again).build().unwrap();
    assert_eq!(policy.links().len(), 2 * N);
    let keys: std::collections::HashSet<PathBuf> =
        policy.links().iter().map(|f| path_key(&f.path)).collect();
    assert_eq!(keys.len(), 2 * N, "a finding kept twice");
    let matter = canon(&w.at("matter"));
    assert_eq!(policy.link_folders(), [matter.join("a"), matter.join("b")]);
}
