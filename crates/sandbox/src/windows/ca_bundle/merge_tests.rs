//! A firm's CA files merged into the machine bundle.

use super::merge::{merge, merged_name};
use super::tests::{INTERMEDIATE, OTHER, ROOT, der_of, pem_of};
use crate::windows::sids::Sid;

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

#[test]
fn the_firm_certificate_sits_beside_the_machine_roots() {
    let machine = pem_of(ROOT);
    let merged = merge(machine.as_bytes(), &[pem_of(OTHER).into_bytes()]);
    assert_eq!(merged.firm_kept, 1, "{merged:?}");
    assert_eq!(merged.pem, format!("{}{}", pem_of(ROOT), pem_of(OTHER)));
}

#[test]
fn a_firm_file_may_hold_several_certificates_or_one_der() {
    let machine = pem_of(ROOT);
    let several = format!(
        "issued by the firm\n{}\n{}",
        pem_of(INTERMEDIATE),
        pem_of(OTHER)
    );
    let merged = merge(machine.as_bytes(), &[several.into_bytes()]);
    assert_eq!(merged.firm_kept, 2, "{merged:?}");
    let der = merge(machine.as_bytes(), &[der_of(OTHER)]);
    assert_eq!(der.firm_kept, 1, "{der:?}");
    assert!(der.pem.ends_with(&pem_of(OTHER)));
}

#[test]
fn each_certificate_is_listed_once() {
    let machine = format!("{}{}", pem_of(ROOT), pem_of(ROOT));
    let merged = merge(
        machine.as_bytes(),
        &[
            pem_of(ROOT).into_bytes(),
            pem_of(OTHER).into_bytes(),
            pem_of(OTHER).into_bytes(),
        ],
    );
    assert_eq!(merged.firm_kept, 1, "{merged:?}");
    assert_eq!(merged.pem, format!("{}{}", pem_of(ROOT), pem_of(OTHER)));
}

#[test]
fn what_doesnt_parse_is_left_out_and_counted() {
    let machine = pem_of(ROOT);
    let broken = "-----BEGIN CERTIFICATE-----\nnot base64 at all\n-----END CERTIFICATE-----\n";
    let merged = merge(
        machine.as_bytes(),
        &[broken.as_bytes().to_vec(), b"garbage".to_vec(), Vec::new()],
    );
    assert_eq!(
        (merged.firm_kept, merged.firm_unparsable),
        (0, 2),
        "{merged:?}"
    );
    assert_eq!(
        merged.pem,
        pem_of(ROOT),
        "the machine bundle stays as it was"
    );
}

#[test]
fn the_merged_name_follows_the_bundle_and_the_pool() {
    let pool = [sid("S-1-5-21-1-2-3-1005"), sid("S-1-5-21-1-2-3-1006")];
    let reversed = [pool[1].clone(), pool[0].clone()];
    let name = merged_name("pem", &pool);
    assert!(
        name.starts_with("roots-") && name.len() == 26 && name.get(22..) == Some(".pem"),
        "{name}"
    );
    assert_eq!(
        name,
        merged_name("pem", &reversed),
        "the pool's order doesn't matter"
    );
    assert_ne!(name, merged_name("pem2", &pool));
    assert_ne!(name, merged_name("pem", &pool[..1]));
}
