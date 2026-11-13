//! Tracked changes in a Word file: `list_revisions` and its answer, `decide_revisions` and its
//! shape checks, with the wire names the app reads.

use clauseo_protocol::{
    CommandOutcome, DecideRevisions, DocVersion, HostCommand, HostQuery, MAX_REVISIONS,
    QueryResult, Revision, RevisionDecision, RevisionKind, RevisionList, Timestamp,
};

use super::fixtures::{checkpoint_id, matter_id, path, round_trips};
use super::{Validate, invalid};

const VERSION: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn decide(ids: Vec<u32>) -> HostCommand {
    HostCommand::DecideRevisions(DecideRevisions {
        matter: matter_id(1),
        path: path("Lease.docx"),
        version: VERSION.parse().unwrap(),
        ids,
        decision: RevisionDecision::Accept,
        all: false,
    })
}

fn decide_all(ids: Vec<u32>) -> HostCommand {
    let HostCommand::DecideRevisions(one) = decide(ids) else {
        unreachable!("decide makes a DecideRevisions");
    };
    HostCommand::DecideRevisions(DecideRevisions { all: true, ..one })
}

#[test]
fn list_revisions_and_its_answer_on_the_wire() {
    let query = HostQuery::ListRevisions {
        matter: matter_id(1),
        path: path("Lease.docx"),
    };
    round_trips(&query);
    assert_eq!(
        serde_json::to_string(&query).unwrap(),
        concat!(
            r#"{"list_revisions":{"matter":"01900000-0000-7000-8000-000000000001","#,
            r#""path":"Lease.docx"}}"#
        )
    );
    // A query with a field it does not know is refused, as every query is.
    let extra = concat!(
        r#"{"list_revisions":{"matter":"01900000-0000-7000-8000-000000000001","#,
        r#""path":"Lease.docx","x":1}}"#
    );
    assert!(serde_json::from_str::<HostQuery>(extra).is_err());
    let short = r#"{"list_revisions":{"matter":"01900000-0000-7000-8000-000000000001"}}"#;
    assert!(serde_json::from_str::<HostQuery>(short).is_err());

    let answer = QueryResult::Revisions(RevisionList {
        path: path("Lease.docx"),
        version: VERSION.parse().unwrap(),
        revisions: vec![
            Revision {
                ids: vec![2, 3],
                kind: RevisionKind::Insert,
                author: "C".to_owned(),
                at: Some(Timestamp::new(1_794_242_630_000).unwrap()),
                block: "p4".to_owned(),
                text: Some("New para.".to_owned()),
            },
            Revision {
                ids: vec![7],
                kind: RevisionKind::Format,
                author: "C".to_owned(),
                at: None,
                block: "p5".to_owned(),
                text: None,
            },
        ],
        truncated: false,
        total: 2,
    });
    round_trips(&answer);
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        concat!(
            r#"{"revisions":{"path":"Lease.docx","version":""#,
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
            r#"","revisions":[{"ids":[2,3],"kind":"insert","author":"C","at":1794242630000,"#,
            r#""block":"p4","text":"New para."},{"ids":[7],"kind":"format","author":"C","#,
            r#""block":"p5"}],"truncated":false,"total":2}}"#
        )
    );
    // A later field still decodes here, and a kind from a newer Clauseo reads as unknown.
    let later = concat!(
        r#"{"revisions":{"path":"a.docx","version":""#,
        "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        r#"","revisions":[{"ids":[1],"kind":"comment","author":"","block":"p1","x":1}],"#,
        r#""truncated":true,"total":600,"more":1}}"#
    );
    let QueryResult::Revisions(list) = serde_json::from_str::<QueryResult>(later).unwrap() else {
        panic!("not revisions");
    };
    assert_eq!(list.revisions[0].kind, RevisionKind::Unknown);
    assert!(list.truncated);
    assert_eq!(list.total, 600);
}

#[test]
fn a_version_is_64_lowercase_hex_characters() {
    let version: DocVersion = VERSION.parse().unwrap();
    assert_eq!(version.to_string(), VERSION);
    assert_eq!(DocVersion::from_bytes(*version.as_bytes()), version);
    for bad in [
        "",
        &VERSION[..62],
        &format!("{VERSION}00"),
        &VERSION.to_uppercase(),
        &format!("{}zz", &VERSION[..62]),
    ] {
        assert!(bad.parse::<DocVersion>().is_err(), "{bad}");
        let json = format!("\"{bad}\"");
        assert!(serde_json::from_str::<DocVersion>(&json).is_err(), "{bad}");
    }
}

#[test]
fn decide_revisions_has_its_json_and_its_outcome() {
    let command = decide(vec![1, 2]);
    round_trips(&command);
    assert!(command.validate().is_ok());
    assert_eq!(
        serde_json::to_string(&command).unwrap(),
        concat!(
            r#"{"decide_revisions":{"matter":"01900000-0000-7000-8000-000000000001","#,
            r#""path":"Lease.docx","version":""#,
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
            r#"","ids":[1,2],"decision":"accept"}}"#
        )
    );
    let json = serde_json::to_string(&command).unwrap();
    let extra = json.replace(r#""decision":"accept""#, r#""decision":"accept","x":1"#);
    assert!(serde_json::from_str::<HostCommand>(&extra).is_err());
    let unknown = json.replace(r#""decision":"accept""#, r#""decision":"maybe""#);
    assert!(serde_json::from_str::<HostCommand>(&unknown).is_err());

    let outcome = CommandOutcome::RevisionsDecided {
        before: checkpoint_id(3),
        checkpoint: Some(checkpoint_id(4)),
    };
    round_trips(&outcome);
    assert_eq!(
        serde_json::to_string(&outcome).unwrap(),
        concat!(
            r#"{"revisions_decided":{"before":"01900000-0000-7000-8000-000000000003","#,
            r#""checkpoint":"01900000-0000-7000-8000-000000000004"}}"#
        )
    );
    // No checkpoint after the write: the key is left out, and read back as none.
    let unchecked = CommandOutcome::RevisionsDecided {
        before: checkpoint_id(3),
        checkpoint: None,
    };
    round_trips(&unchecked);
    assert_eq!(
        serde_json::to_string(&unchecked).unwrap(),
        r#"{"revisions_decided":{"before":"01900000-0000-7000-8000-000000000003"}}"#
    );
}

#[test]
fn decide_revisions_names_one_to_the_most_ids_none_twice() {
    assert!(invalid(&decide(Vec::new()).validate()));
    assert!(invalid(&decide(vec![4, 9, 4]).validate()));
    let most: Vec<u32> = (0..u32::try_from(MAX_REVISIONS).unwrap()).collect();
    assert!(decide(most).validate().is_ok());
    let over: Vec<u32> = (0..=u32::try_from(MAX_REVISIONS).unwrap()).collect();
    assert!(invalid(&decide(over).validate()));
}

#[test]
fn decide_revisions_all_names_no_ids() {
    let all = decide_all(Vec::new());
    round_trips(&all);
    assert!(all.validate().is_ok());
    assert_eq!(
        serde_json::to_string(&all).unwrap(),
        concat!(
            r#"{"decide_revisions":{"matter":"01900000-0000-7000-8000-000000000001","#,
            r#""path":"Lease.docx","version":""#,
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
            r#"","ids":[],"decision":"accept","all":true}}"#
        )
    );
    // All with ids named is refused: the host would not know which the lawyer meant.
    assert!(invalid(&decide_all(vec![1]).validate()));
    // Without "all", the field is left out and read back as false.
    let some = serde_json::to_string(&decide(vec![1])).unwrap();
    assert!(!some.contains("\"all\""));
}
