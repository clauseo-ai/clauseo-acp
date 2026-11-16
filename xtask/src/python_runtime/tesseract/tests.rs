use super::super::lock::Lock;
use super::super::target::{TARGETS, Target};
use super::super::tesseract_build::{
    LEPTONICA_ARGS, TESSERACT_ARGS, foreign_libraries, hardening_args, pe_flag_problems,
};
use super::super::{LOCK, NOTICES, notices, rules};
use super::{bin_dir, exe, leptonica_url, pins, source_url, tessdata_url};
use crate::workspace::workspace_root;

fn real_lock() -> (Lock, String) {
    let text = std::fs::read_to_string(workspace_root().join(LOCK)).expect("read the lock");
    (Lock::parse(&text).expect("the lock parses"), text)
}

/// The real lock with `change` applied must fail with a problem containing `expected`.
fn fails_with(change: impl FnOnce(&mut Lock), expected: &str) {
    let (mut lock, _) = real_lock();
    change(&mut lock);
    let found = rules::problems(&lock);
    assert!(
        found.iter().any(|problem| problem.contains(expected)),
        "expected a problem with `{expected}`, got {found:#?}"
    );
}

#[test]
fn the_lock_ships_english_and_hindi_fast() {
    let (lock, _) = real_lock();
    let t = &lock.tesseract;
    assert_eq!(t.languages, ["eng", "hin"]);
    let files: Vec<String> = pins(t).into_iter().map(|pin| pin.url).collect();
    assert_eq!(
        files,
        [
            source_url(&t.version),
            leptonica_url(&t.leptonica),
            tessdata_url(&t.tessdata_commit, "LICENSE"),
            tessdata_url(&t.tessdata_commit, "eng.traineddata"),
            tessdata_url(&t.tessdata_commit, "hin.traineddata"),
        ]
    );
    assert_eq!(
        source_url("5.5.4"),
        "https://github.com/tesseract-ocr/tesseract/archive/refs/tags/5.5.4.tar.gz"
    );
    assert_eq!(
        leptonica_url("1.87.0"),
        "https://github.com/DanBloomberg/leptonica/releases/download/1.87.0/\
         leptonica-1.87.0.tar.gz"
    );
}

#[test]
fn the_program_goes_beside_pandoc() {
    for target in TARGETS {
        let pandoc = super::super::pandoc::dest(target);
        let folder = pandoc.rsplit_once('/').map(|(folder, _)| folder);
        assert_eq!(folder, Some(bin_dir(target)), "{target:?}");
    }
    assert_eq!(exe(Target::MacX64), "tesseract");
    assert_eq!(exe(Target::WinArm64), "tesseract.exe");
}

#[test]
fn a_bad_tesseract_entry_fails() {
    fails_with(
        |lock| lock.tesseract.version = "5.5.4-rc1".to_owned(),
        "tesseract.version is `5.5.4-rc1`",
    );
    fails_with(
        |lock| lock.tesseract.tessdata_commit = "8741641".to_owned(),
        "tesseract.tessdata_commit is not a 40-digit",
    );
    fails_with(
        |lock| lock.tesseract.languages = vec!["hin".to_owned(), "eng".to_owned()],
        "tesseract.languages is empty, not sorted",
    );
    fails_with(
        |lock| {
            lock.tesseract.models.remove("hin");
        },
        "one entry per language",
    );
    fails_with(
        |lock| lock.tesseract.leptonica_sha256.clear(),
        "no sha256 or size for leptonica-1.87.0.tar.gz",
    );
    fails_with(
        |lock| {
            if let Some(model) = lock.tesseract.models.get_mut("eng") {
                model.size = 0;
            }
        },
        "eng.traineddata",
    );
}

#[test]
fn notices_carry_tesseract_leptonica_and_the_models() {
    let (lock, text) = real_lock();
    let file = std::fs::read_to_string(workspace_root().join(NOTICES)).expect("read the notices");
    for line in [
        "- tesseract 5.5.4 (Apache-2.0)",
        "- leptonica 1.87.0 (BSD-2-Clause)",
        "- tessdata_fast 8741641 (Apache-2.0)",
    ] {
        assert!(file.lines().any(|l| l == line), "{line}");
        let without = file.replace(line, "- (gone)");
        assert!(
            notices::problems(&without, &lock, &text)
                .iter()
                .any(|problem| problem.contains(line)),
            "{line}"
        );
    }
    assert!(file.contains(&lock.tesseract.source_sha256));
    assert!(file.contains("Copyright (C) 2001-2020 Leptonica.  All rights reserved."));
    let pointer = file.replace(&leptonica_url(&lock.tesseract.leptonica), "(none)");
    assert!(
        notices::problems(&pointer, &lock, &text)
            .iter()
            .any(|problem| problem.contains("leptonica-1.87.0.tar.gz"))
    );
}

#[test]
fn the_recipe_turns_every_extra_off() {
    for codec in ["ZLIB", "PNG", "GIF", "JPEG", "TIFF", "WEBP", "OPENJPEG"] {
        let flag = format!("-DENABLE_{codec}=OFF");
        assert!(LEPTONICA_ARGS.contains(&flag.as_str()), "{flag}");
    }
    for flag in [
        "-DBUILD_TRAINING_TOOLS=OFF",
        "-DDISABLE_ARCHIVE=ON",
        "-DDISABLE_CURL=ON",
        "-DDISABLE_TIFF=ON",
        "-DENABLE_OPENCL=OFF",
        "-DWIN32_MT_BUILD=ON",
    ] {
        assert!(TESSERACT_ARGS.contains(&flag), "{flag}");
    }
}

#[test]
fn otool_output_names_foreign_libraries() {
    let clean = "/rt/bin/tesseract:\n\t/usr/lib/libc++.1.dylib (compatibility version 1.0.0)\n\
                 \t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)\n";
    assert_eq!(foreign_libraries(clean), Vec::<String>::new());
    let brew = format!("{clean}\t/opt/homebrew/opt/libarchive/lib/libarchive.13.dylib (x)\n");
    assert_eq!(
        foreign_libraries(&brew),
        ["/opt/homebrew/opt/libarchive/lib/libarchive.13.dylib"]
    );
}

#[test]
fn every_target_is_built_hardened() {
    let args = |target| hardening_args(target).join(" ");
    for target in [Target::WinArm64, Target::WinX64] {
        assert!(
            args(target).contains("-DCMAKE_C_FLAGS_INIT=/guard:cf"),
            "{target:?}"
        );
        assert!(
            args(target).contains("-DCMAKE_CXX_FLAGS_INIT=/guard:cf"),
            "{target:?}"
        );
        assert!(
            args(target).contains("-DCMAKE_EXE_LINKER_FLAGS_INIT=/guard:cf"),
            "{target:?}"
        );
    }
    assert!(args(Target::WinX64).contains("/CETCOMPAT"));
    // ARM64 has no CET, and link.exe rejects /CETCOMPAT there.
    assert!(!args(Target::WinArm64).contains("/CETCOMPAT"));
    for target in [Target::MacArm64, Target::MacX64] {
        for flag in ["-D_FORTIFY_SOURCE=2", "-fstack-protector-strong"] {
            assert!(args(target).contains(flag), "{target:?} {flag}");
        }
    }
}

/// A minimal PE32+ image: `DllCharacteristics` `flags`, and with `ex_flags` a debug directory
/// holding one `EX_DLLCHARACTERISTICS` entry in a section at RVA 0x1000, file offset 0x200.
fn pe_image(flags: u16, ex_flags: Option<u32>) -> Vec<u8> {
    let mut image = vec![0u8; 0x400];
    let put = |image: &mut Vec<u8>, at: usize, bytes: &[u8]| {
        image[at..at + bytes.len()].copy_from_slice(bytes);
    };
    let header = 0x80;
    put(&mut image, 0, b"MZ");
    put(
        &mut image,
        0x3C,
        &u32::try_from(header).unwrap().to_le_bytes(),
    );
    put(&mut image, header, b"PE\0\0");
    put(&mut image, header + 6, &1u16.to_le_bytes());
    put(&mut image, header + 20, &240u16.to_le_bytes());
    let optional = header + 24;
    put(&mut image, optional, &0x20Bu16.to_le_bytes());
    put(&mut image, optional + 70, &flags.to_le_bytes());
    let section = optional + 240;
    put(&mut image, section + 8, &0x200u32.to_le_bytes());
    put(&mut image, section + 12, &0x1000u32.to_le_bytes());
    put(&mut image, section + 20, &0x200u32.to_le_bytes());
    if let Some(ex) = ex_flags {
        put(&mut image, optional + 112 + 48, &0x1000u32.to_le_bytes());
        put(&mut image, optional + 116 + 48, &28u32.to_le_bytes());
        put(&mut image, 0x200 + 12, &20u32.to_le_bytes());
        put(&mut image, 0x200 + 16, &4u32.to_le_bytes());
        put(&mut image, 0x200 + 24, &0x300u32.to_le_bytes());
        put(&mut image, 0x300, &ex.to_le_bytes());
    }
    image
}

#[test]
fn the_pe_check_wants_aslr_dep_cfg_and_cet_on_x64() {
    let all = 0x4000 | 0x100 | 0x40 | 0x20;
    assert_eq!(
        pe_flag_problems(&pe_image(all, Some(1)), Target::WinX64),
        Vec::<String>::new()
    );
    assert_eq!(
        pe_flag_problems(&pe_image(all, None), Target::WinArm64),
        Vec::<String>::new()
    );
    let no_cet = pe_flag_problems(&pe_image(all, None), Target::WinX64);
    assert_eq!(no_cet, ["CET_COMPAT is off (no /CETCOMPAT)"]);
    assert_eq!(
        pe_flag_problems(&pe_image(all, Some(0)), Target::WinX64),
        no_cet
    );
    let no_cfg = pe_flag_problems(&pe_image(all & !0x4000, Some(1)), Target::WinX64);
    assert_eq!(no_cfg.len(), 1);
    assert!(no_cfg[0].starts_with("GUARD_CF is off"), "{no_cfg:?}");
    let bare = pe_flag_problems(&pe_image(0, None), Target::WinArm64);
    assert_eq!(bare.len(), 4, "{bare:?}");
    let unreadable = ["its PE headers can't be read"];
    assert_eq!(pe_flag_problems(b"MZ", Target::WinX64), unreadable);
    assert_eq!(
        pe_flag_problems(&pe_image(all, Some(1))[..0x100], Target::WinX64),
        unreadable
    );
}
