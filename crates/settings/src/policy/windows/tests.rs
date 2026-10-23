//! These read the real registry, never write it. The keys besides Clauseo's are ones every
//! Windows has.

use super::{reg_value, values, wide_strings};
use crate::policy::registry::{KEY, RegValue};
use crate::policy::{PolicyRead, system_policy_sources};
use windows::Win32::System::Registry::{REG_BINARY, REG_DWORD, REG_MULTI_SZ, REG_SZ};

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
