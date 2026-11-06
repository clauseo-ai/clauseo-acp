//! The hint's table: queries and result URLs that must fire, each pattern at least once, and
//! ones that must not (the brief's four among them).

use clauseo_protocol::{AgentId, SearchSource, WebResult, WebSearchRecorded};

use super::fires;
use crate::pipeline::tests::support::id;

/// A web search record of `query` with one result at each of `urls`.
fn record(query: &str, urls: &[&str]) -> WebSearchRecorded {
    let results = urls
        .iter()
        .map(|url| WebResult {
            title: "A page".to_owned(),
            url: (*url).to_owned(),
        })
        .collect();
    WebSearchRecorded::new(
        id(1),
        id(2),
        AgentId::new("claude-acp").unwrap(),
        query,
        results,
    )
}

/// What fires, and the pattern it stands for.
const FIRE: [(&str, &[&str], &str); 22] = [
    ("(2017) 10 SCC 1 right to privacy", &[], "SCC"),
    ("SCC Online Kesavananda", &[], "SCC at the start"),
    ("AIR 1973 SC 1461", &[], "AIR"),
    ("[1950] SCR 88 Gopalan", &[], "SCR"),
    ("2024 INSC 12 arbitration", &[], "INSC, the neutral form"),
    ("2023:DHC:1234 bail order", &[], "neutral YYYY:COURT:N"),
    ("Kesavananda Bharati v. State of Kerala", &[], "v."),
    ("Puttaswamy V. Union of India", &[], "v. in capitals"),
    ("Arnesh Kumar vs State of Bihar", &[], "vs"),
    ("Lalita Kumari vs. Govt of UP", &[], "vs."),
    (
        "Supreme Court judgment section 138 NI Act",
        &[],
        "the scenario's query",
    ),
    ("supreme court cheque bounce", &[], "supreme court"),
    ("Bombay HIGH COURT anticipatory bail", &[], "high court"),
    ("NCLAT insolvency moratorium", &[], "nclat"),
    ("consumer tribunal deficiency of service", &[], "tribunal"),
    ("landmark judgment on privacy", &[], "judgment"),
    ("judgement on dowry death", &[], "judgement"),
    ("precedent for specific performance", &[], "precedent"),
    (
        "cheque dishonour notice period",
        &["https://indiankanoon.org/doc/257876/"],
        "indiankanoon.org",
    ),
    (
        "cheque dishonour notice",
        &["https://www.IndianKanoon.org/search/?q=x"],
        "www., any case",
    ),
    (
        "case status 138",
        &["https://main.sci.gov.in/case-status"],
        "a seed host's subdomain",
    ),
    (
        "bail conditions",
        &[
            "https://example.com/a",
            "https://delhihighcourt.nic.in/web/",
        ],
        "a seed host",
    ),
];

/// What must not fire.
const QUIET: [(&str, &[&str]); 21] = [
    ("air pollution limits Delhi", &[]),
    ("scc online login help", &[]),
    ("contract template", &[]),
    ("versus meaning", &[]),
    ("GST rate on restaurants", &[]),
    ("how to register a company in India", &[]),
    ("rental agreement stamp duty Maharashtra", &[]),
    ("AIRPORT parking charges", &[]),
    ("SCCM software licence", &[]),
    ("vsco photo editor", &[]),
    ("v.s. naipaul novels", &[]),
    ("income tax return due date", &[]),
    ("court fee calculator", &[]),
    ("the tribune newspaper", &[]),
    ("2023:dhc:1234", &[]),
    ("1899:DHC:12", &[]),
    ("insc membership", &[]),
    ("notice period law", &["https://example.com/contract"]),
    ("labour code", &["https://notsci.gov.in/"]),
    (
        "labour code wages",
        &["https://indiankanoon.org.example.com/x"],
    ),
    ("minimum wages", &["ftp://indiankanoon.org/doc/1/"]),
];

#[test]
fn each_pattern_fires() {
    for (query, urls, pattern) in FIRE {
        assert!(fires(&record(query, urls)), "{pattern}: {query} {urls:?}");
    }
}

#[test]
fn the_others_do_not_fire() {
    for (query, urls) in QUIET {
        assert!(!fires(&record(query, urls)), "{query} {urls:?}");
    }
}

/// The connector's own records never offer the connector.
#[test]
fn a_case_law_record_does_not_fire() {
    let own = record("Supreme Court judgment section 138 NI Act", &[])
        .with_source(SearchSource::CaseLaw, Some("search_law"));
    assert!(!fires(&own));
}
