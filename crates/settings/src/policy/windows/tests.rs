//! These read the real registry, never write it. The keys besides Clauseo's are ones every
//! Windows has.

use super::{NoProgramData, reg_value, values, wide_strings};
use crate::Effective;
use crate::error::Error;
use crate::leaf;
use crate::policy::registry::{KEY, RegValue};
use crate::policy::{PolicyRead, system_policy_sources};
use crate::testing::{MemFiles, locations};
use crate::warning::{Origin, WarningKind};
use windows::Win32::System::Registry::{REG_BINARY, REG_DWORD, REG_MULTI_SZ, REG_SZ};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};

#[test]
fn system_policy_absent_on_clean_machine() {
    let sources = system_policy_sources();
    let names: Vec<_> = sources
        .iter()
        .map(|source| source.name().to_owned())
        .collect();
    assert_eq!(names[0], "registry");
    assert!(names[1].ends_with(r"\Clauseo\policy.json"), "{names:?}");
    for source in &sources {
        assert!(
            matches!(source.read(), PolicyRead::Absent),
            "{}",
            source.name()
        );
    }
    assert_eq!(values(KEY).unwrap(), None);
}

#[test]
fn registry_shim_reads_sz_and_dword_values() {
    let values = values(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .unwrap()
        .unwrap();
    assert!(
        values
            .iter()
            .any(|(name, value)| name == "CurrentMajorVersionNumber"
                && matches!(value, RegValue::Dword(major) if *major >= 10)),
        "{values:?}"
    );
    assert!(values.iter().any(|(name, value)| name == "ProductName"
        && matches!(value, RegValue::Sz(product) if product.starts_with("Windows"))));
}

#[test]
fn registry_shim_reads_multi_sz_values() {
    let values = values(r"SYSTEM\CurrentControlSet\Control\ServiceGroupOrder")
        .unwrap()
        .unwrap();
    assert!(values.iter().any(|(name, value)| name == "List"
        && matches!(value, RegValue::MultiSz(groups) if groups.len() > 1)));
}

#[test]
fn a_missing_program_data_folder_is_reported_by_its_name() {
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&NoProgramData]);
    assert!(effective.policy().is_absent());
    let errors = effective.policy().errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].source, r"ProgramData\Clauseo\policy.json");
    assert!(
        matches!(
            errors[0].error,
            Error::NoFolder {
                folder: "ProgramData"
            }
        ),
        "{errors:?}"
    );
    // Fails closed: each security key is locked at its default, with a warning naming the source.
    for key in leaf::SECURITY_KEYS {
        assert!(effective.is_locked(key), "{key}");
        assert!(
            effective.warnings().iter().any(|warning| warning.origin
                == Origin::Store {
                    name: r"ProgramData\Clauseo\policy.json".to_owned()
                }
                && matches!(&warning.kind, WarningKind::InvalidValue { key: locked, .. }
                    if locked == key)),
            "{key}: {:?}",
            effective.warnings()
        );
    }
}

/// The handles this process holds. Nextest runs each test in a process of its own.
fn handle_count() -> u32 {
    let mut count = 0u32;
    // SAFETY: the current process' pseudo-handle needs no closing, and `count` is a valid u32.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &raw mut count) }.unwrap();
    count
}

#[test]
fn registry_shim_closes_each_key_it_opens() {
    let key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    // The first read may open handles that stay, such as HKLM's own.
    values(key).unwrap().unwrap();
    let before = handle_count();
    for _ in 0..64 {
        values(key).unwrap().unwrap();
    }
    let after = handle_count();
    // A key left open costs one handle per read: 64 more.
    assert!(
        after < before + 32,
        "{before} handles before 64 reads, {after} after"
    );
}

#[test]
fn registry_shim_finds_no_missing_key() {
    assert_eq!(
        values(r"SOFTWARE\Policies\Clauseo\NoSuchKey").unwrap(),
        None
    );
}

fn wide(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[test]
fn registry_shim_decodes_each_type() {
    assert_eq!(reg_value(REG_DWORD.0, &[7, 1, 0, 0]), RegValue::Dword(263));
    assert_eq!(
        reg_value(REG_DWORD.0, &[7, 1]),
        RegValue::Other(REG_DWORD.0)
    );
    assert_eq!(
        reg_value(REG_SZ.0, &wide("dark\0")),
        RegValue::Sz("dark".to_owned())
    );
    assert_eq!(reg_value(REG_SZ.0, &[]), RegValue::Sz(String::new()));
    assert_eq!(
        reg_value(REG_MULTI_SZ.0, &wide("a\0b c\0\0")),
        RegValue::MultiSz(vec!["a".to_owned(), "b c".to_owned()])
    );
    assert_eq!(reg_value(REG_BINARY.0, &[1]), RegValue::Other(REG_BINARY.0));
    assert_eq!(wide_strings(&wide("one")), ["one"]);
}
