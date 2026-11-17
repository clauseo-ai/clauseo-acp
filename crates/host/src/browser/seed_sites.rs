//! The sites every matter's allowlist starts from: official Indian legal sites (S27, "Consent,
//! in every tier"; research 25:515-517). Stage C's proxy reads this list (S16), and R21 seeds
//! one Claude `WebFetch(domain:<host>)` allow rule per host of [`WEB_FETCH_SITES`] (this list and
//! [`FETCH_ONLY_SITES`]) while `tools.web_search` is on. There is no bare `WebFetch` allow: a
//! fetch from any other host goes to the permission mode, so Anthropic's auto-mode check judges it
//! in auto mode and a card asks in an ask mode (decision record 0030, "Claude's web fetches").
//! Text the agent puts in a URL to a seeded site reaches that site's operator (up to 2,000
//! characters per fetch, any number of fetches); this list limits where fetches go, not what
//! they carry. It covers `WebFetch` only, not the shell, nor Claude Code's own preapproved
//! developer-documentation hosts.
//!
//! Claude Code 2.1.293 matches a `domain:` rule with no `*` against the URL's host exactly,
//! lower-cased and without a trailing dot: `domain:sci.gov.in` does not cover `www.sci.gov.in`
//! (`*.x` would cover subdomains of `x` but not `x`, and no rule here uses a wildcard). A
//! redirect that only adds or drops `www.` is followed inside one fetch, but a `www.` URL the
//! model has from a search result is a different host, so [`FETCH_ONLY_SITES`] lists the `www.`
//! form of each seeded host that resolves.
//!
//! The High Courts are each court's own site, never a portal such as eCourts. Each host was
//! checked to resolve with `getent hosts` on 2026-11-03 (decision record 0029, "Seed and deny
//! lists"); the unit tests never touch the network.

use std::sync::LazyLock;

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

/// The hosts Claude may also fetch unasked, beyond [`SEED_SITES`]: the `www.` form of seeded
/// hosts, and the sites of a lawyer's ordinary research day that the browser's list leaves out.
/// Only Claude's `WebFetch` rules read it; the browser's proxy and the connectors hint keep
/// [`SEED_SITES`]. Each host resolved with `getent hosts`, and through 1.1.1.1 and 8.8.8.8, on
/// 2026-11-16. Left out because they did not: `www.indiankanoon.org`,
/// `www.gujarathighcourt.nic.in`, `www.hcraj.nic.in`, and `main.sci.gov.in`, which `sci.gov.in`'s
/// own name servers (`ns1.nic.in` and the rest) answer NXDOMAIN.
pub const FETCH_ONLY_SITES: &[&str] = &[
    // The Supreme Court: its site as it serves it, and the host of its judgment PDFs.
    "www.sci.gov.in",
    "api.sci.gov.in",
    // The High Courts' own sites, `www.` form.
    "www.allahabadhighcourt.in",
    "www.aphc.gov.in",
    "www.bombayhighcourt.nic.in",
    "www.calcuttahighcourt.gov.in",
    "www.highcourt.cg.gov.in",
    "www.delhihighcourt.nic.in",
    "www.ghconline.gov.in",
    "www.hphighcourt.nic.in",
    "www.jkhighcourt.nic.in",
    "www.jharkhandhighcourt.nic.in",
    "www.judiciary.karnataka.gov.in",
    "www.highcourt.kerala.gov.in",
    "www.mphc.gov.in",
    "www.hcmadras.tn.gov.in",
    "www.hcmimphal.nic.in",
    "www.meghalayahighcourt.nic.in",
    "www.orissahighcourt.nic.in",
    "www.patnahighcourt.gov.in",
    "www.highcourtchd.gov.in",
    "www.hcs.gov.in",
    "www.tshc.gov.in",
    "www.thc.nic.in",
    "www.highcourtofuttarakhand.gov.in",
    // eCourts: case status, cause lists, orders and the judgment search.
    "ecourts.gov.in",
    "www.ecourts.gov.in",
    "services.ecourts.gov.in",
    "hcservices.ecourts.gov.in",
    "judgments.ecourts.gov.in",
    "njdg.ecourts.gov.in",
    // Indian Kanoon, the public judgment search most research starts from.
    "indiankanoon.org",
    // Supreme Court Observer, a non-official case tracker (on the Indian Kanoon and PRS precedent
    // for non-official research sites).
    "scobserver.in",
    "www.scobserver.in",
    // Legislation: India Code, the Legislative Department, Parliament, PRS, the Law Commission.
    "www.indiacode.nic.in",
    "legislative.gov.in",
    "www.legislative.gov.in",
    "lddashboard.legislative.gov.in",
    "sansad.in",
    "www.sansad.in",
    "eparlib.sansad.in",
    "prsindia.org",
    "www.prsindia.org",
    "lawcommissionofindia.nic.in",
    "www.lawcommissionofindia.nic.in",
    // The e-Gazette.
    "www.egazette.gov.in",
    // The regulators the legal instructions name (CCI, SEBI, RBI, TRAI, IBBI, MCA).
    "www.cci.gov.in",
    "www.sebi.gov.in",
    "www.rbi.org.in",
    "rbidocs.rbi.org.in",
    "trai.gov.in",
    "www.trai.gov.in",
    "www.ibbi.gov.in",
    "www.mca.gov.in",
    // The tribunals: NCLT, NCLAT, ITAT, NGT, CESTAT, DRT and DRAT, SAT, APTEL, TDSAT, CAT,
    // NCDRC (CCI is above).
    "nclt.gov.in",
    "www.nclt.gov.in",
    "nclat.gov.in",
    "www.nclat.gov.in",
    "nclat.nic.in",
    "www.nclat.nic.in",
    "itat.gov.in",
    "www.itat.gov.in",
    "greentribunal.gov.in",
    "www.greentribunal.gov.in",
    "cestat.gov.in",
    "www.cestat.gov.in",
    "drt.gov.in",
    "www.drt.gov.in",
    "sat.gov.in",
    "www.sat.gov.in",
    "aptel.gov.in",
    "www.aptel.gov.in",
    "tdsat.gov.in",
    "www.tdsat.gov.in",
    "cgat.gov.in",
    "ncdrc.nic.in",
    "e-jagriti.gov.in",
];

/// Every host seeded as a Claude `WebFetch(domain:<host>)` allow rule: [`SEED_SITES`], then
/// [`FETCH_ONLY_SITES`].
pub static WEB_FETCH_SITES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| SEED_SITES.iter().chain(FETCH_ONLY_SITES).copied().collect());

#[cfg(test)]
mod tests {
    use super::{FETCH_ONLY_SITES, SEED_SITES, WEB_FETCH_SITES};

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

    #[test]
    fn the_fetch_list_is_the_seed_list_then_the_fetch_only_hosts_each_once() {
        assert_eq!(WEB_FETCH_SITES[..SEED_SITES.len()], *SEED_SITES);
        assert_eq!(WEB_FETCH_SITES[SEED_SITES.len()..], *FETCH_ONLY_SITES);
        let mut sorted = WEB_FETCH_SITES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), WEB_FETCH_SITES.len(), "a host listed twice");
    }

    /// Each fetch host is a plain domain: no wildcard, no scheme, no port, no path, no query,
    /// lower case. Claude Code matches a `domain:` rule without `*` exactly, and one with `*`
    /// as a pattern, so a stray character would widen or break the rule.
    #[test]
    fn each_fetch_host_is_a_plain_domain() {
        for host in WEB_FETCH_SITES.iter() {
            assert!(
                !host.contains(['*', '/', ':', '?', '#', '@', ' ']),
                "{host} is not a plain domain"
            );
            let labels: Vec<&str> = host.split('.').collect();
            assert!(labels.len() >= 2, "{host}");
            assert!(
                labels.iter().all(|label| !label.is_empty()
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')),
                "{host} is not a plain lowercase domain"
            );
            assert!(
                labels.last().is_some_and(|tld| ["in", "org"].contains(tld)),
                "{host}: an Indian site or one of the .org legal sources"
            );
        }
    }
}
