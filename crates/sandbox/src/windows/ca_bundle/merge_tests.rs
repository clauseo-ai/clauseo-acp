//! A firm's CA files merged into the machine bundle.

use super::merge::{merge, merged_name};
use super::tests::{INTERMEDIATE, OTHER, ROOT, der_of, pem_of};
use crate::windows::sids::Sid;

/// A self-signed end entity, `CN=Clauseo Test leaf`, basic constraints `cA` false: no CA.
const LEAF: &str = "\
    3082016a30820111a00302010202145d56fde3f46e2b3c688b8081c9800375b7259043300a06082a8648ce3d\
    040302301c311a301806035504030c11436c617573656f2054657374206c6561663020170d32363131303730\
    37313333315a180f32313236313031343037313333315a301c311a301806035504030c11436c617573656f20\
    54657374206c6561663059301306072a8648ce3d020106082a8648ce3d03010703420004077ceb57f71fc1f0\
    b475789770be9f045cc32a786aad190a9478bdf6475fb59bcf6cfe1daba325548cae651e041f3650d152a55d\
    0ca49c534526be97485000aca32f302d301d0603551d0e04160414a9a247f03a1786cb792c4c9758137e6918\
    89fdd2300c0603551d130101ff04023000300a06082a8648ce3d04030203470030440220428611778bd47604\
    e6168ba030ef16df63f926a731d8c8b5f99b19a89a9359f6022036142a81708ecf18de57a996e042cce2fb52\
    e6f5d9027c518159356bc755efc3";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

#[test]
fn the_firm_certificate_sits_beside_the_machine_roots() {
    let machine = pem_of(ROOT);
    let merged = merge(machine.as_bytes(), &[pem_of(OTHER).into_bytes()], &[]);
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
    let merged = merge(machine.as_bytes(), &[several.into_bytes()], &[]);
    assert_eq!(merged.firm_kept, 2, "{merged:?}");
    let der = merge(machine.as_bytes(), &[der_of(OTHER)], &[]);
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
        &[],
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
        &[],
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

#[test]
fn a_disallowed_or_non_ca_firm_certificate_is_left_out() {
    let machine = pem_of(ROOT);
    let firm = vec![pem_of(OTHER).into_bytes(), pem_of(LEAF).into_bytes()];
    let merged = merge(machine.as_bytes(), &firm, &[der_of(OTHER)]);
    assert_eq!(
        (merged.firm_kept, merged.firm_disallowed, merged.firm_not_ca),
        (0, 1, 1),
        "{merged:?}"
    );
    assert_eq!(merged.pem, pem_of(ROOT), "only the machine roots");
    let kept = merge(machine.as_bytes(), &firm, &[]);
    assert_eq!((kept.firm_kept, kept.firm_not_ca), (1, 1), "{kept:?}");
    assert!(super::der::is_ca(&der_of(OTHER)) && !super::der::is_ca(&der_of(LEAF)));
}
