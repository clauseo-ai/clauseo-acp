//! The connector's research-question estimates at the view: passed on as sent, absent when the
//! connector sends none.

use std::time::SystemTime;

use clauseo_connectors::{Account, CASE_LAW, CredentialKind, CredentialStatus, Liveness, Pricing};
use clauseo_protocol::{ConnectorStatus, PackView, ResearchQuestionBasis, ResearchQuestionView};

use super::{CaseLaw, account, at, fixture_pricing};

/// The connector's research-question estimates reach the view as sent: each pack's, one
/// question's (its `gross_inr` in paise) and the balance's. From an older connector that sends
/// none, each is `None`, never 0. A recharge's balance drops the account's estimate, which was
/// for the old balance, until the next account read.
#[test]
fn the_research_question_estimates_pass_through_unchanged() {
    let with: Pricing = serde_json::from_str(include_str!(
        "../../../../../connectors/fixtures/pricing-research-questions.json"
    ))
    .unwrap();
    let liveness = |pricing: Pricing| Liveness {
        live: true,
        pricing: Some(pricing),
        fetched_at: Some(SystemTime::UNIX_EPOCH),
    };
    let mut state = CaseLaw {
        liveness: Some(liveness(with)),
        ..CaseLaw::default()
    };
    state.set_credential(CredentialStatus::SignedIn(CredentialKind::OAuth));
    state.set_account(
        Account {
            research_questions: Some(5),
            research_questions_text: Some("about 5 research questions".to_owned()),
            ..account(2_000, "200.00")
        },
        at(1_000),
    );
    let view = |state: &CaseLaw| state.view(&CASE_LAW, true, false).connectors[0].clone();
    let balance = |state: &CaseLaw| {
        let status = view(state).status;
        let ConnectorStatus::SignedIn {
            balance: Some(balance),
            ..
        } = status
        else {
            panic!("{status:?}");
        };
        balance
    };
    let connector = view(&state);
    let pack = |name: &str| {
        connector
            .packs
            .iter()
            .find(|pack| pack.pack == name)
            .unwrap()
            .clone()
    };
    assert_eq!(
        pack("200"),
        PackView {
            pack: "200".to_owned(),
            credits: 200,
            paise: 2_360,
            research_questions: Some(0),
            research_questions_text: Some("less than one research question".to_owned()),
        }
    );
    assert_eq!(pack("2000").research_questions, Some(5));
    assert_eq!(
        pack("2000").research_questions_text.as_deref(),
        Some("about 5 research questions")
    );
    assert_eq!(
        connector.research_question,
        Some(ResearchQuestionView {
            credits: 400,
            paise: Some(4_720),
            basis: Some(ResearchQuestionBasis {
                new_search_pages: 10,
                repeat_search_pages: 5,
                judgment_opens: 30,
                read_credits: 60,
                citator_pages: 4,
            }),
        })
    );
    let shown = balance(&state);
    assert_eq!(shown.research_questions, Some(5));
    assert_eq!(
        shown.research_questions_text.as_deref(),
        Some("about 5 research questions")
    );
    // The page shape: the names the UI reads.
    let json = serde_json::to_value(&connector).unwrap();
    assert_eq!(json["research_question"]["paise"], 4_720);
    assert_eq!(json["research_question"]["basis"]["citator_pages"], 4);
    assert_eq!(
        json["packs"][0]["research_questions_text"],
        "less than one research question"
    );
    assert_eq!(
        json["status"]["signed_in"]["balance"]["research_questions_text"],
        "about 5 research questions"
    );

    state.set_balance(2_200, at(2_000));
    let shown = balance(&state);
    assert_eq!(shown.credits, 2_200);
    assert_eq!(
        (shown.research_questions, shown.research_questions_text),
        (None, None)
    );

    // An older connector: no estimate anywhere, and none on the wire.
    state.liveness = Some(liveness(fixture_pricing()));
    state.set_account(account(2_000, "200.00"), at(3_000));
    let connector = view(&state);
    assert_eq!(connector.research_question, None);
    for pack in &connector.packs {
        assert_eq!(
            (pack.research_questions, &pack.research_questions_text),
            (None, &None)
        );
    }
    let shown = balance(&state);
    assert_eq!(
        (shown.research_questions, shown.research_questions_text),
        (None, None)
    );
    let json = serde_json::to_string(&connector).unwrap();
    assert!(!json.contains("research_question"), "{json}");
}

/// A research-question text reaches the page only as one plain line of at most 80 characters,
/// on a pack and on the balance alike; a refused text is `None` and its number stays.
#[test]
fn a_research_question_text_is_kept_only_as_one_short_plain_line() {
    let eighty = "a".repeat(80);
    let eighty_one = "a".repeat(81);
    for (text, kept) in [
        ("about 5\nresearch questions", false),
        ("about 5\rresearch questions", false),
        (eighty_one.as_str(), false),
        ("about 5 research questions\u{0007}", false),
        (eighty.as_str(), true),
    ] {
        let mut pricing = fixture_pricing();
        pricing.packs[0].research_questions = Some(5);
        pricing.packs[0].research_questions_text = Some(text.to_owned());
        let mut state = CaseLaw {
            liveness: Some(Liveness {
                live: true,
                pricing: Some(pricing),
                fetched_at: Some(SystemTime::UNIX_EPOCH),
            }),
            ..CaseLaw::default()
        };
        state.set_credential(CredentialStatus::SignedIn(CredentialKind::OAuth));
        state.set_account(
            Account {
                research_questions: Some(5),
                research_questions_text: Some(text.to_owned()),
                ..account(2_000, "200.00")
            },
            at(1_000),
        );
        let connector = state.view(&CASE_LAW, true, false).connectors[0].clone();
        let want = kept.then(|| text.to_owned());
        let pack = &connector.packs[0];
        assert_eq!(pack.research_questions, Some(5), "{text:?}");
        assert_eq!(pack.research_questions_text, want, "{text:?}");
        let ConnectorStatus::SignedIn {
            balance: Some(balance),
            ..
        } = connector.status
        else {
            panic!("{:?}", connector.status);
        };
        assert_eq!(balance.research_questions, Some(5), "{text:?}");
        assert_eq!(balance.research_questions_text, want, "{text:?}");
    }
}
