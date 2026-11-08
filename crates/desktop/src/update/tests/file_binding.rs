//! The file binding (`file_binding.rs`): the expected name per platform, pinned to release.yml,
//! and the check over signatures a key made here signs (never a real key), plus the `tauri
//! signer` fixture's own prehashed signature.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use clauseo_host::UpdaterError;
use clauseo_settings::Locations;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair as _};

use super::super::arch::Target;
use super::super::file_binding::{
    self, FileBindingError, MAC_NAME, MacName, check_update_file, expected_update_file, field,
    installer_arch, mac_archive_arch, update_file_name,
};
use super::{crate_dir, test_key_file};
use crate::test_support::Scratch;

/// The update's version in these tests.
const VERSION: &str = "0.4.1";
/// This version's x64 installer, as the release signs it.
const X64: &str = "Clauseo_0.4.1_x64-setup.exe";

/// A minisign key made for one test.
pub(in crate::update) struct TestKey {
    pair: Ed25519KeyPair,
    id: [u8; 8],
}

impl TestKey {
    pub(in crate::update) fn new() -> TestKey {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        TestKey {
            pair: Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap(),
            id: 0x5EED_F11E_B1ED_0001_u64.to_le_bytes(),
        }
    }

    /// The public key in Tauri's form: base64 of the minisign `.pub` text.
    pub(in crate::update) fn public(&self) -> String {
        let mut line = b"Ed".to_vec();
        line.extend_from_slice(&self.id);
        line.extend_from_slice(self.pair.public_key().as_ref());
        let id = u64::from_le_bytes(self.id);
        STANDARD.encode(format!(
            "untrusted comment: minisign public key: {id:016X}\n{}\n",
            STANDARD.encode(line)
        ))
    }

    /// The minisign `.sig` text of `data` with the trusted comment `comment` (a legacy `Ed`
    /// signature: no `BLAKE2b` in the tree; the fixture covers the prehashed kind).
    fn sig_text(&self, data: &[u8], comment: &str) -> String {
        let signature = self.pair.sign(data);
        let mut line = b"Ed".to_vec();
        line.extend_from_slice(&self.id);
        line.extend_from_slice(signature.as_ref());
        let mut global = signature.as_ref().to_vec();
        global.extend_from_slice(comment.as_bytes());
        let global = self.pair.sign(&global);
        format!(
            "untrusted comment: signature from tauri secret key\n{}\n\
             trusted comment: {comment}\n{}\n",
            STANDARD.encode(line),
            STANDARD.encode(global.as_ref())
        )
    }

    /// [`Self::sig_text`] in Tauri's form: base64 of it.
    pub(in crate::update) fn sign(&self, data: &[u8], comment: &str) -> String {
        STANDARD.encode(self.sig_text(data, comment))
    }
}

/// The trusted comment `tauri signer sign --app-version` writes.
pub(in crate::update) fn comment(file: &str, version: &str) -> String {
    format!("timestamp:1762500000\tfile:{file}\tversion:{version}")
}

/// `bytes` signed as `file` at `version`, checked against the x64 installer of [`VERSION`].
fn check_signed(file: &str, version: &str) -> Result<(), FileBindingError> {
    let key = TestKey::new();
    let bytes = b"an installer";
    let signature = key.sign(bytes, &comment(file, version));
    check_update_file(&key.public(), &signature, bytes, X64, VERSION)
}

#[test]
fn each_platform_expects_its_own_installer_name() {
    assert_eq!(installer_arch("x86_64"), Some("x64"));
    assert_eq!(installer_arch("aarch64"), Some("arm64"));
    assert_eq!(installer_arch("x86"), None);
    assert_eq!(
        update_file_name(MacName::Bundle, "windows", "x86_64", VERSION).as_deref(),
        Some(X64)
    );
    assert_eq!(
        update_file_name(MacName::Bundle, "windows", "aarch64", VERSION).as_deref(),
        Some("Clauseo_0.4.1_arm64-setup.exe")
    );
    assert_eq!(
        update_file_name(MacName::Bundle, "windows", "x86", VERSION),
        None
    );
    for arch in ["x86_64", "aarch64"] {
        assert_eq!(
            update_file_name(MacName::Bundle, "macos", arch, VERSION).as_deref(),
            Some("Clauseo.app.tar.gz")
        );
        assert_eq!(
            update_file_name(MacName::Bundle, "linux", arch, VERSION),
            None
        );
    }
    assert_eq!(
        expected_update_file(VERSION, Target::Native),
        update_file_name(
            MAC_NAME,
            std::env::consts::OS,
            std::env::consts::ARCH,
            VERSION
        )
    );
}

/// The names are the ones the release signs: `productName`, release.yml's Windows matrix arch
/// names, the installer it signs with `--app-version`, and the macOS archive the bundler signs.
#[test]
fn the_expected_names_are_the_ones_the_release_signs() {
    let conf = std::fs::read_to_string(crate_dir().join("tauri.conf.json")).unwrap();
    let conf: serde_json::Value = serde_json::from_str(&conf).unwrap();
    assert_eq!(conf["productName"], "Clauseo");
    let workflow = crate_dir().join("../../.github/workflows/release.yml");
    let release = std::fs::read_to_string(workflow).unwrap();
    for needle in [
        "{ target: x86_64-pc-windows-msvc, runner: windows-2025, tier: 1, arch: x64 }",
        "{ target: aarch64-pc-windows-msvc, runner: windows-11-arm, tier: 2, arch: arm64 }",
        r#"tauri signer sign --app-version "$VERSION" "$OUT/Clauseo_${VERSION}_${{ matrix.arch }}-setup.exe""#,
        r#"cp "$bundle/macos/Clauseo.app.tar.gz" "$bundle/macos/Clauseo.app.tar.gz.sig" "$OUT/""#,
    ] {
        assert!(
            release.contains(needle),
            "release.yml no longer has {needle}"
        );
    }
}

#[test]
fn the_right_file_at_the_right_version_passes() {
    assert_eq!(check_signed(X64, VERSION), Ok(()));
}

/// The release signs the sandbox tool and the other architecture's installer with the same key
/// at the same version: each verifies, and each is refused as another file.
#[test]
fn a_validly_signed_other_file_is_refused() {
    for other in [
        "clauseo-sandbox-setup.exe",
        "clauseo-sandbox-setup-x64.exe",
        "Clauseo_0.4.1_arm64-setup.exe",
        "Clauseo_0.4.1_x64-machine-setup.exe",
        "Clauseo.app.tar.gz",
        "clauseo_0.4.1_x64-setup.exe",
        "Clauseo_0.4.1_x64-setup.exe ",
    ] {
        assert_eq!(
            check_signed(other, VERSION),
            Err(FileBindingError::WrongFile {
                found: other.to_owned()
            }),
            "{other}"
        );
    }
}

/// An arm64 build refuses the x64 installer, though signed for its version by the key.
#[test]
fn the_other_architectures_installer_is_refused() {
    let key = TestKey::new();
    let bytes = b"an installer";
    let signature = key.sign(bytes, &comment(X64, VERSION));
    let arm64 = update_file_name(MacName::Bundle, "windows", "aarch64", VERSION).unwrap();
    assert_eq!(
        check_update_file(&key.public(), &signature, bytes, &arm64, VERSION),
        Err(FileBindingError::WrongFile {
            found: X64.to_owned()
        })
    );
}

/// An older release's installer is refused: under its own name, and under this version's name
/// with its older signed version.
#[test]
fn an_older_versions_installer_is_refused() {
    assert_eq!(
        check_signed("Clauseo_0.4.0_x64-setup.exe", "0.4.0"),
        Err(FileBindingError::WrongFile {
            found: "Clauseo_0.4.0_x64-setup.exe".to_owned()
        })
    );
    assert_eq!(
        check_signed(X64, "0.4.0"),
        Err(FileBindingError::WrongVersion {
            found: "0.4.0".to_owned()
        })
    );
    assert_eq!(
        check_signed(X64, "v0.4.1"),
        Err(FileBindingError::WrongVersion {
            found: "v0.4.1".to_owned()
        })
    );
}

#[test]
fn a_missing_or_doubled_field_is_refused() {
    let key = TestKey::new();
    let bytes = b"an installer";
    let cases = [
        (
            "timestamp:1762500000\tversion:0.4.1",
            FileBindingError::NoFile,
        ),
        (
            "timestamp:1762500000\tfile:\tversion:0.4.1",
            FileBindingError::WrongFile {
                found: String::new(),
            },
        ),
        (
            "timestamp:1762500000\tfile:Clauseo_0.4.1_x64-setup.exe\tfile:clauseo-sandbox-setup.exe\tversion:0.4.1",
            FileBindingError::NoFile,
        ),
        (
            "timestamp:1762500000 file:Clauseo_0.4.1_x64-setup.exe version:0.4.1",
            FileBindingError::NoFile,
        ),
        (
            "timestamp:1762500000\tfile:Clauseo_0.4.1_x64-setup.exe",
            FileBindingError::NoVersion,
        ),
        (
            "timestamp:1762500000\tfile:Clauseo_0.4.1_x64-setup.exe\tversion:0.4.1\tversion:0.4.0",
            FileBindingError::NoVersion,
        ),
        ("", FileBindingError::NoFile),
    ];
    for (comment, refused) in cases {
        let signature = key.sign(bytes, comment);
        assert_eq!(
            check_update_file(&key.public(), &signature, bytes, X64, VERSION),
            Err(refused),
            "{comment:?}"
        );
    }
}

/// A trusted comment changed after signing (to name the right file) fails the global signature,
/// so its fields are never read; so do changed bytes and another key.
#[test]
fn an_unverified_signature_is_refused() {
    let key = TestKey::new();
    let bytes = b"the sandbox tool";
    let signed = key.sig_text(bytes, &comment("clauseo-sandbox-setup.exe", VERSION));
    let tampered = signed.replace("clauseo-sandbox-setup.exe", X64);
    assert_ne!(tampered, signed);
    let tampered = STANDARD.encode(tampered);
    assert_eq!(
        check_update_file(&key.public(), &tampered, bytes, X64, VERSION),
        Err(FileBindingError::Unverified)
    );

    let signature = key.sign(bytes, &comment(X64, VERSION));
    assert_eq!(
        check_update_file(&key.public(), &signature, b"other bytes", X64, VERSION),
        Err(FileBindingError::Unverified)
    );
    let other = TestKey::new();
    assert_eq!(
        check_update_file(&other.public(), &signature, bytes, X64, VERSION),
        Err(FileBindingError::Unverified)
    );
    for (key, signature) in [
        ("not base64!", signature.as_str()),
        (&key.public(), "bm90IGEgc2ln"),
    ] {
        assert_eq!(
            check_update_file(key, signature, bytes, X64, VERSION),
            Err(FileBindingError::Unreadable)
        );
    }
}

/// `tauri signer`'s own prehashed (`ED`) signature over `payload.bin` (`fixtures/`, signed as
/// `file:payload.bin`, `version:0.4.1`): it verifies, and names no installer.
#[test]
fn the_tauri_signer_fixture_verifies_and_names_its_own_file() {
    let fixtures = crate_dir().join("src/update/fixtures");
    let key = std::fs::read_to_string(fixtures.join("fixture.pub")).unwrap();
    let payload = std::fs::read(fixtures.join("payload.bin")).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixtures.join("latest.json")).unwrap()).unwrap();
    let signature = manifest["platforms"]["windows-x86_64"]["signature"]
        .as_str()
        .unwrap();
    assert_eq!(
        check_update_file(&key, signature, &payload, "payload.bin", VERSION),
        Ok(())
    );
    assert_eq!(
        check_update_file(&key, signature, &payload, X64, VERSION),
        Err(FileBindingError::WrongFile {
            found: "payload.bin".to_owned()
        })
    );
}

/// The adapter's gate: this platform's installer passes; another file is the host's signature
/// failure, logged on the lifecycle target with both names and no signature. `install` sets the
/// process's global subscriber; nextest runs each test in its own process.
#[test]
fn the_gate_refuses_another_file_and_logs_both_names() {
    let scratch = Scratch::folder("update-binding-log", &[]);
    let locations = Locations::under_home(scratch.path());
    let log = clauseo_host::log_file::install(&locations);
    let key = TestKey::new();
    let bytes = b"an installer";
    let sandbox = key.sign(bytes, &comment("clauseo-sandbox-setup.exe", VERSION));
    let refused = file_binding::bind(
        Some(&key.public()),
        &sandbox,
        VERSION,
        bytes,
        Target::Native,
    );
    let Some(expected) = expected_update_file(VERSION, Target::Native) else {
        // Linux installs no update: every file is refused.
        assert!(
            matches!(refused, Err(UpdaterError::Signature(_))),
            "{refused:?}"
        );
        return;
    };
    assert_eq!(
        refused,
        Err(UpdaterError::Signature(
            "the signature is for another file than this platform's installer".to_owned()
        ))
    );
    let right = key.sign(bytes, &comment(&expected, VERSION));
    assert_eq!(
        file_binding::bind(Some(&key.public()), &right, VERSION, bytes, Target::Native),
        Ok(())
    );
    assert!(matches!(
        file_binding::bind(None, &right, VERSION, bytes, Target::Native),
        Err(UpdaterError::Signature(_))
    ));
    log.flush();
    let text = std::fs::read_to_string(locations.log_dir().join("host.log")).unwrap();
    let line = text
        .lines()
        .find(|line| line.contains("does not name this platform's installer"))
        .unwrap_or_else(|| panic!("no refusal in {text}"));
    assert!(line.contains("clauseo_desktop::lifecycle"), "{line}");
    assert!(
        line.contains(&expected) && line.contains("clauseo-sandbox-setup.exe"),
        "{line}"
    );
    assert!(!text.contains(&sandbox) && !text.contains(&right), "{text}");
    let sig_line = key.sig_text(bytes, &comment("clauseo-sandbox-setup.exe", VERSION));
    let raw = sig_line.lines().nth(1).unwrap();
    assert!(!text.contains(raw), "{text}");
}

/// macOS's two forms: today's bundler name for both architectures (in force), and the per-arch
/// asset name stage F's release will sign once it signs after renaming. Windows and Linux are the
/// same under both.
#[test]
fn macos_names_its_archive_in_either_form_and_the_bundle_form_is_in_force() {
    assert_eq!(MAC_NAME, MacName::Bundle);
    assert_eq!(mac_archive_arch("aarch64"), Some("aarch64"));
    assert_eq!(mac_archive_arch("x86_64"), Some("x64"));
    assert_eq!(mac_archive_arch("x86"), None);
    for (arch, per_arch) in [
        ("aarch64", "Clauseo_0.4.1_aarch64.app.tar.gz"),
        ("x86_64", "Clauseo_0.4.1_x64.app.tar.gz"),
    ] {
        assert_eq!(
            update_file_name(MacName::Bundle, "macos", arch, VERSION).as_deref(),
            Some("Clauseo.app.tar.gz")
        );
        assert_eq!(
            update_file_name(MacName::PerArch, "macos", arch, VERSION).as_deref(),
            Some(per_arch)
        );
        for os in ["windows", "linux"] {
            assert_eq!(
                update_file_name(MacName::PerArch, os, arch, VERSION),
                update_file_name(MacName::Bundle, os, arch, VERSION)
            );
        }
    }
    assert_eq!(
        update_file_name(MacName::PerArch, "macos", "x86", VERSION),
        None
    );
    // Under the per-arch form, the other architecture's archive is refused.
    let key = TestKey::new();
    let bytes = b"an archive";
    let x64 = key.sign(bytes, &comment("Clauseo_0.4.1_x64.app.tar.gz", VERSION));
    let aarch64 = update_file_name(MacName::PerArch, "macos", "aarch64", VERSION).unwrap();
    assert_eq!(
        check_update_file(&key.public(), &x64, bytes, &aarch64, VERSION),
        Err(FileBindingError::WrongFile {
            found: "Clauseo_0.4.1_x64.app.tar.gz".to_owned()
        })
    );
}

/// A real `.sig` from `tests/fixtures/updater/`: its trusted comment, after its global signature
/// was verified against the test key (`release/updater-test.pub`, which signed the test-v0.0.3
/// pre-release these come from). The artifacts are not in the repo, so the file's own signature
/// cannot be checked here; `minisign-verify` checks the global signature only together with it,
/// so it is checked with ring over the same bytes minisign signs: the signature, then the comment.
pub(super) fn real_trusted_comment(name: &str) -> String {
    let path = crate_dir().join("tests/fixtures/updater").join(name);
    let text = decode(std::fs::read_to_string(path).unwrap().trim());
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{name}: not minisign's signature text");
    let raw = STANDARD.decode(lines[1]).unwrap();
    let comment = lines[2].strip_prefix("trusted comment: ").unwrap();
    let global = STANDARD.decode(lines[3]).unwrap();
    let parsed = minisign_verify::Signature::decode(&text).unwrap();
    assert_eq!(parsed.trusted_comment(), comment, "{name}");
    let key_text = decode(&test_key_file());
    let key = STANDARD.decode(key_text.lines().nth(1).unwrap()).unwrap();
    assert_eq!((raw.len(), key.len()), (74, 42), "{name}");
    assert_eq!(&raw[..2], b"ED", "{name}: tauri signer signs prehashed");
    assert_eq!(raw[2..10], key[2..10], "{name}: signed by another key");
    let verify = |comment: &str| {
        let message = [&raw[10..], comment.as_bytes()].concat();
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &key[10..])
            .verify(&message, &global)
            .is_ok()
    };
    assert!(
        verify(comment),
        "{name}: the global signature does not hold"
    );
    let swapped = comment.replace(
        "file:clauseo-sandbox-setup.exe",
        "file:Clauseo_0.0.3_x64-setup.exe",
    );
    if swapped != comment {
        assert!(!verify(&swapped), "{name}: a changed comment verified");
    }
    comment.to_owned()
}

/// Base64 text, decoded as UTF-8.
fn decode(text: &str) -> String {
    String::from_utf8(STANDARD.decode(text).unwrap()).unwrap()
}

/// The real test-v0.0.3 signatures: each installer names exactly the file `update_file_name`
/// expects for its architecture at its version, and the sandbox tool's names neither, so both
/// Windows builds refuse it. No macOS fixture yet: stage F is changing the signed Mac names.
#[test]
fn the_real_release_signatures_name_the_expected_files() {
    let x64 = update_file_name(MAC_NAME, "windows", "x86_64", "0.0.3").unwrap();
    let arm64 = update_file_name(MAC_NAME, "windows", "aarch64", "0.0.3").unwrap();
    for (fixture, expected) in [
        ("real-Clauseo_0.0.3_x64-setup.exe.sig", &x64),
        ("real-Clauseo_0.0.3_arm64-setup.exe.sig", &arm64),
    ] {
        let comment = real_trusted_comment(fixture);
        assert_eq!(
            field(&comment, "file"),
            Some(expected.as_str()),
            "{comment}"
        );
        assert_eq!(field(&comment, "version"), Some("0.0.3"), "{comment}");
        assert!(comment.starts_with("timestamp:"), "{comment}");
    }
    let sandbox = real_trusted_comment("real-clauseo-sandbox-setup-x64.exe.sig");
    let named = field(&sandbox, "file");
    assert_eq!(named, Some("clauseo-sandbox-setup.exe"), "{sandbox}");
    assert_eq!(field(&sandbox, "version"), Some("0.0.3"), "{sandbox}");
    for expected in [&x64, &arm64] {
        assert_ne!(named, Some(expected.as_str()));
    }
}
