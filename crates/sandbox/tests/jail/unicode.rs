//! A matter named in Devanagari (Hindi content is a hard requirement). APFS opens a name in any
//! Unicode form and Seatbelt matches its rules across forms, so the jail lets the agent write
//! through every form of the matter's path and refuses every form of a denied one. The matter
//! is made in NFD and the policy built from its NFC path.

use std::io::ErrorKind;
use std::path::PathBuf;

use crate::support::{Output, World, launcher, run, try_dir_at};

/// `क़ानून ऩ` with क़ as U+0958, a composition exclusion: NFC and NFD both decompose it.
const RAW: &str = "\u{0958}\u{093E}\u{0928}\u{0942}\u{0928} \u{0929}";
/// [`RAW`] in NFC: क़ decomposed, ऩ (U+0929) composed.
const NFC: &str = "\u{0915}\u{093C}\u{093E}\u{0928}\u{0942}\u{0928} \u{0929}";
/// [`RAW`] in NFD: both nukta letters decomposed.
const NFD: &str = "\u{0915}\u{093C}\u{093E}\u{0928}\u{0942}\u{0928} \u{0928}\u{093C}";
/// `अनुबंध ऴ`, a subfolder's name, in NFC (ऴ composed) and NFD.
const SUB_NFC: &str = "\u{0905}\u{0928}\u{0941}\u{092C}\u{0902}\u{0927} \u{0934}";
const SUB_NFD: &str = "\u{0905}\u{0928}\u{0941}\u{092C}\u{0902}\u{0927} \u{0933}\u{093C}";
/// `ग़ुप्त ऩऱ`, a hidden folder's name: raw (ग़ as U+095A), NFC and NFD.
const HIDDEN_RAW: &str = "\u{095A}\u{0941}\u{092A}\u{094D}\u{0924} \u{0929}\u{0931}";
const HIDDEN_NFC: &str = "\u{0917}\u{093C}\u{0941}\u{092A}\u{094D}\u{0924} \u{0929}\u{0931}";
const HIDDEN_NFD: &str =
    "\u{0917}\u{093C}\u{0941}\u{092A}\u{094D}\u{0924} \u{0928}\u{093C}\u{0930}\u{093C}";

/// Perl that creates the file `$ARGV[0]` and prints `DONE`.
const TOUCH: &str = r#"open(my $f, '>', $ARGV[0]) or die "open: $!\n"; close $f; print "DONE\n";"#;

/// Perl that reads the file `$ARGV[0]` and prints it.
const READ: &str = r#"open(my $f, '<', $ARGV[0]) or die "open: $!\n"; print <$f>;"#;

/// The world with its matter made in [`NFD`].
fn devanagari_world() -> World {
    let w = World::new();
    w.dir(NFD);
    w
}

/// `perl -e script path` through `launcher`, in the matter.
fn perl(
    launcher: &dyn clauseo_process::Launcher,
    w: &World,
    script: &str,
    path: &std::path::Path,
) -> Output {
    let path = path.to_str().unwrap();
    run(
        launcher,
        "/usr/bin/perl",
        &["-e", script, path],
        &w.at(NFD),
        &[],
    )
}

#[test]
fn a_write_through_every_form_of_the_matter_succeeds() {
    let w = devanagari_world();
    let policy = w.builder_for(NFC).build().unwrap();
    assert_eq!(
        policy.matter(),
        w.at(NFD),
        "the policy keeps the stored form"
    );
    let (launcher, _) = launcher(policy);
    for (i, form) in [NFC, NFD, RAW].into_iter().enumerate() {
        let file = w.at(form).join(format!("draft-{i}.txt"));
        assert_eq!(
            perl(&launcher, &w, TOUCH, &file).assert_ok().stdout,
            "DONE\n"
        );
    }
    let mut made: Vec<String> = std::fs::read_dir(w.at(NFD))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    made.sort();
    assert_eq!(made, ["draft-0.txt", "draft-1.txt", "draft-2.txt"]);
}

#[test]
fn a_nested_claude_md_is_refused_through_every_form_of_its_folder() {
    let w = devanagari_world();
    w.dir(&format!("{NFD}/{SUB_NFC}"));
    let (launcher, _) = launcher(w.builder_for(NFC).build().unwrap());
    for matter in [NFC, NFD, RAW] {
        for sub in [SUB_NFD, SUB_NFC] {
            let file = w.at(matter).join(sub).join("CLAUDE.md");
            perl(&launcher, &w, TOUCH, &file).assert_refused();
        }
    }
    assert!(!w.at(NFD).join(SUB_NFC).join("CLAUDE.md").exists());
    // Another file in the folder, by its other form, is a plain write.
    let notes = w.at(NFC).join(SUB_NFD).join("notes.txt");
    perl(&launcher, &w, TOUCH, &notes).assert_ok();
}

#[test]
fn a_hidden_devanagari_folder_is_refused_in_every_form() {
    let w = devanagari_world();
    w.file(&format!("{HIDDEN_NFC}/secret.txt"), "hidden\n");
    let policy = w.builder_for(NFC).hidden(w.at(HIDDEN_NFD)).build().unwrap();
    let (launcher, _) = launcher(policy);
    for form in [HIDDEN_RAW, HIDDEN_NFC, HIDDEN_NFD] {
        let folder = w.at(form);
        let read = perl(&launcher, &w, READ, &folder.join("secret.txt"));
        read.assert_refused();
        assert!(!read.stdout.contains("hidden"), "{read:#?}");
        perl(&launcher, &w, TOUCH, &folder.join("new.txt")).assert_refused();
    }
}

#[test]
fn a_name_with_one_more_nukta_is_outside_the_matter() {
    let w = devanagari_world();
    let beside = format!("{NFD}\u{093C}");
    // A different name, so the volume makes it beside the matter.
    w.dir(&beside);
    let (launcher, _) = launcher(w.builder_for(NFC).build().unwrap());
    perl(&launcher, &w, TOUCH, &w.at(&beside).join("x.txt")).assert_refused();
    assert!(!w.at(&beside).join("x.txt").exists());
}

#[test]
fn no_sibling_can_differ_from_the_matter_only_in_form() {
    let w = devanagari_world();
    for form in [NFC, RAW] {
        let error = try_dir_at(&w.at(form)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::AlreadyExists, "{form:?}: {error}");
    }
    let names: Vec<PathBuf> = std::fs::read_dir(w.at(""))
        .unwrap()
        .map(|entry| PathBuf::from(entry.unwrap().file_name()))
        .filter(|name| name.to_str().is_some_and(|s| !s.is_ascii()))
        .collect();
    assert_eq!(
        names,
        [PathBuf::from(NFD)],
        "APFS keeps the form the folder was made in"
    );
}
