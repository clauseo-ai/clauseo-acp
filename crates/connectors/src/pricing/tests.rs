use super::*;

const LIVE: &[u8] = include_bytes!("../../fixtures/pricing-live.json");
const PREVIEW: &[u8] = include_bytes!("../../fixtures/pricing-preview.json");
const MCP: &str = "https://data.clauseo.chat/mcp/case-law";

fn at() -> SystemTime {
    SystemTime::UNIX_EPOCH + std::time::Duration::from_hours(498_216)
}

#[test]
fn the_live_fixture_parses_with_its_packs() {
    let doc = parse_pricing(LIVE).unwrap();
    let pricing = &doc.pricing;
    assert_eq!(pricing.status, PricingStatus::Live);
    assert_eq!(pricing.currency, "INR");
    assert_eq!(pricing.credit_inr, "0.10");
    assert_eq!(pricing.gst_rate, "0.18");
    assert_eq!(pricing.prices["search_page"], 20);
    assert_eq!(pricing.prices["cite_resolve"], 5);
    assert_eq!(pricing.prices.len(), 7);
    let names: Vec<&str> = pricing
        .packs
        .iter()
        .map(|pack| pack.pack.as_str())
        .collect();
    assert_eq!(names, ["200", "500", "1000", "2000", "5000"]);
    assert_eq!(
        pricing.packs[0],
        Pack {
            pack: "200".to_owned(),
            credits: 200,
            net_inr: "20.00".to_owned(),
            gst_inr: "3.60".to_owned(),
            gross_inr: "23.60".to_owned(),
        }
    );
    assert_eq!(pricing.starter_credits_on_claim, Some(1000));
    assert_eq!(doc.endpoint_mcp.as_deref(), Some(MCP));
}

#[test]
fn liveness_is_true_only_for_a_live_200() {
    let live = liveness_from(200, LIVE, MCP, at());
    assert!(live.live);
    assert_eq!(live.pricing.unwrap().packs.len(), 5);
    assert_eq!(live.fetched_at, Some(at()));

    let preview = liveness_from(200, PREVIEW, MCP, at());
    assert!(!preview.live);
    assert_eq!(preview.pricing.unwrap().status, PricingStatus::Preview);

    for (status, body) in [
        (404, LIVE),
        (500, LIVE),
        (200, b"<html>".as_slice()),
        (200, b"[]"),
    ] {
        assert_eq!(
            liveness_from(status, body, MCP, at()),
            Liveness::failed(at())
        );
    }
}

#[test]
fn another_mcp_endpoint_is_ignored() {
    let body = String::from_utf8(LIVE.to_vec()).unwrap().replace(
        "https://data.clauseo.chat/mcp/case-law",
        "https://elsewhere.example/mcp",
    );
    let live = liveness_from(200, body.as_bytes(), MCP, at());
    assert!(live.live);
}

#[test]
fn rupees_take_a_number_and_odd_fields_default() {
    let doc = parse_pricing(
        br#"{"status":"soon","credit_inr":0.25,"gst_rate":null,"prices":{"open":"3","x":"y","y":1.5},
            "packs":[{"pack":200,"credits":"200","net_inr":20,"gross_inr":"23.60","extra":{}},7],
            "starter_credits_on_claim":"many"}"#,
    )
    .unwrap();
    let pricing = doc.pricing;
    assert_eq!(pricing.status, PricingStatus::Other("soon".to_owned()));
    assert_eq!(pricing.credit_inr, "0.25");
    assert_eq!(pricing.gst_rate, "");
    assert_eq!(pricing.currency, "");
    assert_eq!(
        pricing.prices.into_iter().collect::<Vec<_>>(),
        [("open".to_owned(), 3)]
    );
    assert_eq!(pricing.packs.len(), 1);
    assert_eq!(pricing.packs[0].pack, "200");
    assert_eq!(pricing.packs[0].credits, 200);
    assert_eq!(pricing.packs[0].net_inr, "20");
    assert_eq!(pricing.packs[0].gst_inr, "");
    assert_eq!(pricing.starter_credits_on_claim, None);
    assert_eq!(doc.endpoint_mcp, None);
}
