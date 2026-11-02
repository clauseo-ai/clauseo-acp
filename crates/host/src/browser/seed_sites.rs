//! The sites every matter's allowlist starts from: official Indian legal sites (S27, "Consent,
//! in every tier"; research 25:515-517). Stage C's proxy reads this list (S16), and R21 seeds
//! one Claude `WebFetch(domain:<host>)` allow rule per host from it while `tools.web_search` is
//! on. Rules match the exact host name, so each is listed as the site serves it. S27 adds the
//! High Court sites, each by its host name, and checks that each resolves.

/// The seeded hosts, in the order S27 names them.
pub(crate) const SEED_SITES: [&str; 8] = [
    "sci.gov.in",
    "egazette.gov.in",
    "cci.gov.in",
    "sebi.gov.in",
    "rbi.org.in",
    "ibbi.gov.in",
    "mca.gov.in",
    "indiacode.nic.in",
];
