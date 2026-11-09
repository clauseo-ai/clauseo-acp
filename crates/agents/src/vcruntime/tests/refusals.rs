//! The signer's names, the version resource's original name, the no-replace move and the read
//! cap: each refusal with fake Windows checks, on any OS.

use super::fixture::Original;
use super::{Fake, Layout, SHIPPED, WIN_ARM64, outcomes, pe_file};
use crate::vcruntime::pe::MACHINE_ARM64;
use crate::vcruntime::{Outcome, Refused, Signer, judge, place_with, read_pe_within};

const DLL: &str = "vcruntime140.dll";

#[test]
fn only_microsoft_corporation_as_both_names_under_a_microsoft_root_is_microsoft() {
    let ms = "Microsoft Corporation";
    assert_eq!(judge(ms, ms, true), Signer::Microsoft);
    // Microsoft's third-party driver-signing certificate: Microsoft's organisation and root,
    // another common name.
    let driver = judge(
        "Microsoft Windows Hardware Compatibility Publisher",
        ms,
        true,
    );
    assert!(
        matches!(&driver, Signer::Other(who) if who.contains("Hardware Compatibility")),
        "{driver:?}"
    );
    assert!(matches!(judge(ms, "Contoso Ltd", true), Signer::Other(_)));
    assert!(matches!(judge(ms, ms, false), Signer::Other(_)));
    // Exact names: no prefix, suffix or case games.
    assert!(matches!(
        judge("Microsoft Corporation ", ms, true),
        Signer::Other(_)
    ));
    assert!(matches!(
        judge("microsoft corporation", ms, true),
        Signer::Other(_)
    ));
}

#[test]
fn a_dll_signed_with_a_driver_signing_certificate_is_refused() {
    let layout = Layout::usual();
    let driver = judge(
        "Microsoft Windows Hardware Compatibility Publisher",
        "Microsoft Corporation",
        true,
    );
    let fake = Fake::new(driver, Some(SHIPPED));
    let found = outcomes(place_with(WIN_ARM64, layout.root(), &fake));
    assert!(
        matches!(
            found.as_slice(),
            [Outcome::Refused(_, Refused::NotMicrosoft(_))]
        ),
        "{found:?}"
    );
    assert_eq!(layout.names("codex-path"), ["rg.exe"]);
}

#[test]
fn another_microsoft_dll_renamed_is_refused() {
    for (given, refused) in [
        (
            Some("ucrtbase.dll"),
            Refused::WrongName(Some("ucrtbase.dll".to_owned())),
        ),
        (
            Some("vcruntime140_1.dll"),
            Refused::WrongName(Some("vcruntime140_1.dll".to_owned())),
        ),
        (None, Refused::WrongName(None)),
    ] {
        let layout = Layout::usual();
        let mut fake = Fake::good();
        fake.original = Original::Given(given);
        let found = outcomes(place_with(WIN_ARM64, layout.root(), &fake));
        assert_eq!(found, [Outcome::Refused(DLL.to_owned(), refused)]);
        assert_eq!(layout.names("codex-path"), ["rg.exe"]);
    }
    // The name is compared in any case, as Windows' file names are.
    let layout = Layout::usual();
    let mut fake = Fake::good();
    fake.original = Original::Given(Some("VCRUNTIME140.DLL"));
    assert_eq!(
        outcomes(place_with(WIN_ARM64, layout.root(), &fake)),
        [Outcome::Placed(DLL.to_owned(), SHIPPED)]
    );
}

/// A file of the DLL's name that appears after the look beside rg.exe (here, while the copy is
/// checked) is never replaced: the move fails, and the checked copy is removed.
#[test]
fn a_dll_appearing_during_the_checks_is_never_replaced() {
    let layout = Layout::usual();
    let dest = layout.rg_dir("codex-path").join(DLL);
    let mut fake = Fake::good();
    fake.plant = Some(dest.clone());
    let found = outcomes(place_with(WIN_ARM64, layout.root(), &fake));
    assert!(
        matches!(found.as_slice(), [Outcome::Failed(dll, detail)]
            if dll == DLL && detail.starts_with("moving the copy into place")),
        "{found:?}"
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"appeared meanwhile");
    assert_eq!(layout.names("codex-path"), ["rg.exe", DLL]);
}

#[test]
fn a_file_over_the_read_cap_is_refused_not_cut() {
    let layout = Layout::usual();
    let source = layout.source(DLL);
    let len = u64::try_from(std::fs::read(&source).unwrap().len()).unwrap();
    let err = read_pe_within(&source, len - 1).unwrap_err();
    assert!(err.contains("larger than"), "{err}");
    assert_eq!(read_pe_within(&source, len).unwrap().machine, MACHINE_ARM64);
    // A file far over the cap whose first bytes are a whole PE is still refused.
    let mut padded = pe_file(MACHINE_ARM64, (14, 50), &["KERNEL32.dll"], &[]);
    let whole = u64::try_from(padded.len()).unwrap();
    padded.extend_from_slice(&[0; 4096]);
    let big = layout.dir.file("big.dll", &padded);
    assert!(read_pe_within(&big, whole).is_err());
}
