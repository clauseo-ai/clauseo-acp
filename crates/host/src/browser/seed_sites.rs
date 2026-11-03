//! The sites every matter's allowlist starts from: official Indian legal sites (S27, "Consent,
//! in every tier"; research 25:515-517). Stage C's proxy reads this list (S16), and R21 seeds
//! one Claude `WebFetch(domain:<host>)` allow rule per host from it while `tools.web_search` is
//! on. Rules match the exact host name, so each is listed as the site serves it.
//!
//! The High Courts are each court's own site, never a portal such as eCourts. Each host was
//! checked to resolve with `getent hosts` on 2026-11-03 (decision record 0029, "Seed and deny
//! lists"); the unit tests never touch the network.

/// The seeded hosts: the national sites in the order S27 names them, then the 25 High Courts in
/// the alphabetical order of the courts' names.
pub const SEED_SITES: &[&str] = &[
    "sci.gov.in",
    "egazette.gov.in",
    "cci.gov.in",
    "sebi.gov.in",
    "rbi.org.in",
    "ibbi.gov.in",
    "mca.gov.in",
    "indiacode.nic.in",
    // Allahabad
    "allahabadhighcourt.in",
    // Andhra Pradesh
    "aphc.gov.in",
    // Bombay
    "bombayhighcourt.nic.in",
    // Calcutta
    "calcuttahighcourt.gov.in",
    // Chhattisgarh
    "highcourt.cg.gov.in",
    // Delhi
    "delhihighcourt.nic.in",
    // Gauhati
    "ghconline.gov.in",
    // Gujarat
    "gujarathighcourt.nic.in",
    // Himachal Pradesh
    "hphighcourt.nic.in",
    // Jammu & Kashmir and Ladakh
    "jkhighcourt.nic.in",
    // Jharkhand
    "jharkhandhighcourt.nic.in",
    // Karnataka
    "judiciary.karnataka.gov.in",
    // Kerala
    "highcourt.kerala.gov.in",
    // Madhya Pradesh
    "mphc.gov.in",
    // Madras
    "hcmadras.tn.gov.in",
    // Manipur
    "hcmimphal.nic.in",
    // Meghalaya
    "meghalayahighcourt.nic.in",
    // Orissa
    "orissahighcourt.nic.in",
    // Patna
    "patnahighcourt.gov.in",
    // Punjab and Haryana
    "highcourtchd.gov.in",
    // Rajasthan
    "hcraj.nic.in",
    // Sikkim
    "hcs.gov.in",
    // Telangana
    "tshc.gov.in",
    // Tripura
    "thc.nic.in",
    // Uttarakhand
    "highcourtofuttarakhand.gov.in",
];

#[cfg(test)]
mod tests {
    use super::SEED_SITES;

    #[test]
    fn eight_national_sites_and_25_high_courts_each_once() {
        assert_eq!(SEED_SITES.len(), 8 + 25);
        let mut sorted = SEED_SITES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), SEED_SITES.len(), "a host listed twice");
    }

    #[test]
    fn each_is_a_bare_lowercase_host_name() {
        for host in SEED_SITES {
            let labels: Vec<&str> = host.split('.').collect();
            assert!(labels.len() >= 2, "{host}");
            assert!(
                labels.iter().all(|label| !label.is_empty()
                    && !label.starts_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')),
                "{host} is not a bare lowercase host name"
            );
            assert_eq!(labels.last(), Some(&"in"), "{host}: an Indian site");
        }
    }
}
