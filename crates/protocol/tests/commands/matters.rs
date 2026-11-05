//! The matter commands of the rail: `CreateMatter`'s details and its default location,
//! `UpdateMatter`'s missing, `null` and set fields (its skill packs too), `OpenMatter` and
//! `ForgetMatter`, and the matter a `HostError` names.

use clauseo_protocol::{
    ClientName, Date, DefaultLocation, ErrorCode, FieldChange, HostCommand, HostError, HostQuery,
    MatterSource, PackId, PackList, QueryResult,
};

use super::Validate;
use super::fixtures::{matter_id, round_trips, title};

#[test]
fn a_default_location_create_is_a_plain_string_and_follows_the_folder_name_rules() {
    let json = r#"{"create_matter":{"name":"Smith v Jones","source":"default_location"}}"#;
    let command: HostCommand = serde_json::from_str(json).unwrap();
    assert_eq!(
        command,
        HostCommand::CreateMatter {
            name: title("Smith v Jones"),
            source: MatterSource::DefaultLocation,
            client: None,
            next_date: None,
        }
    );
    assert_eq!(serde_json::to_string(&command).unwrap(), json);
    assert!(command.validate().is_ok());
    // A name that is no folder name is refused here as it is for a new folder.
    let bad = HostCommand::CreateMatter {
        name: title("Smith: v Jones"),
        source: MatterSource::DefaultLocation,
        client: None,
        next_date: None,
    };
    assert!(bad.validate().is_err());
}

#[test]
fn a_create_carries_a_trimmed_client_and_a_calendar_date() {
    let json = r#"{"create_matter":{"name":"A","source":"default_location",
        "client":"  Rathi Steels ","next_date":"2026-11-20"}}"#;
    let HostCommand::CreateMatter {
        client, next_date, ..
    } = serde_json::from_str(json).unwrap()
    else {
        panic!("not a create");
    };
    assert_eq!(client.unwrap().as_str(), "Rathi Steels");
    assert_eq!(next_date.unwrap().to_string(), "2026-11-20");
    for bad in [
        r#"{"create_matter":{"name":"A","source":"default_location","client":" "}}"#,
        r#"{"create_matter":{"name":"A","source":"default_location","next_date":"2026-02-30"}}"#,
        r#"{"create_matter":{"name":"A","source":"default_location","court":"High Court"}}"#,
    ] {
        assert!(serde_json::from_str::<HostCommand>(bad).is_err(), "{bad}");
    }
}

#[test]
fn an_update_keeps_a_missing_field_clears_a_null_one_and_sets_a_value() {
    let matter = matter_id(1);
    let decode = |fields: &str| -> HostCommand {
        let json = format!(r#"{{"update_matter":{{"matter":"{matter}"{fields}}}}}"#);
        serde_json::from_str(&json).unwrap()
    };
    assert_eq!(
        decode(""),
        HostCommand::UpdateMatter {
            matter,
            client: FieldChange::Keep,
            next_date: FieldChange::Keep,
            skill_packs: FieldChange::Keep,
            pinned: None,
        }
    );
    assert_eq!(
        decode(r#","client":null,"next_date":"2027-01-04","pinned":false"#),
        HostCommand::UpdateMatter {
            matter,
            client: FieldChange::Clear,
            next_date: FieldChange::Set("2027-01-04".parse::<Date>().unwrap()),
            skill_packs: FieldChange::Keep,
            pinned: Some(false),
        }
    );
    let set = decode(r#","client":"Acme","next_date":null,"pinned":true"#);
    assert_eq!(
        set,
        HostCommand::UpdateMatter {
            matter,
            client: FieldChange::Set(ClientName::new("Acme").unwrap()),
            next_date: FieldChange::Clear,
            skill_packs: FieldChange::Keep,
            pinned: Some(true),
        }
    );
    round_trips(&set);
    round_trips(&decode(""));
    assert!(set.validate().is_ok());
    let unknown = format!(r#"{{"update_matter":{{"matter":"{matter}","note":"x"}}}}"#);
    assert!(serde_json::from_str::<HostCommand>(&unknown).is_err());
    let null_pin = format!(r#"{{"update_matter":{{"matter":"{matter}","pinned":null}}}}"#);
    assert_eq!(
        serde_json::from_str::<HostCommand>(&null_pin).unwrap(),
        decode(""),
        "a null pinned is no change"
    );
}

#[test]
fn an_update_keeps_a_missing_skill_packs_clears_a_null_one_and_sets_a_list() {
    let matter = matter_id(1);
    let json = |fields: &str| format!(r#"{{"update_matter":{{"matter":"{matter}"{fields}}}}}"#);
    let packs = |fields: &str| -> FieldChange<PackList> {
        let HostCommand::UpdateMatter { skill_packs, .. } =
            serde_json::from_str(&json(fields)).unwrap()
        else {
            panic!("not an update");
        };
        skill_packs
    };
    let list = |ids: &[&str]| {
        PackList::new(ids.iter().map(|id| PackId::new(id).unwrap()).collect()).unwrap()
    };
    assert_eq!(packs(""), FieldChange::Keep);
    assert_eq!(packs(r#","skill_packs":null"#), FieldChange::Clear);
    assert_eq!(
        packs(r#","skill_packs":["nda-kit","a1"]"#),
        FieldChange::Set(list(&["nda-kit", "a1"]))
    );
    assert_eq!(packs(r#","skill_packs":[]"#), FieldChange::Set(list(&[])));
    let too_many: Vec<String> = (0..33).map(|at| format!("p{at}")).collect();
    let too_many = format!(
        r#","skill_packs":{}"#,
        serde_json::to_string(&too_many).unwrap()
    );
    for bad in [
        r#","skill_packs":["nda-kit","nda-kit"]"#,
        too_many.as_str(),
        r#","skill_packs":["NDA-kit"]"#,
        r#","skill_packs":["clauseo"]"#,
        r#","skill_packs":"nda-kit""#,
    ] {
        assert!(
            serde_json::from_str::<HostCommand>(&json(bad)).is_err(),
            "{bad}"
        );
    }
    let keep = HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        skill_packs: FieldChange::Keep,
        pinned: None,
    };
    let encoded = serde_json::to_string(&keep).unwrap();
    assert!(!encoded.contains("skill_packs"), "{encoded}");
    for change in [
        FieldChange::Clear,
        FieldChange::Set(list(&[])),
        FieldChange::Set(list(&["nda-kit", "a1"])),
    ] {
        let command = HostCommand::UpdateMatter {
            matter,
            client: FieldChange::Keep,
            next_date: FieldChange::Keep,
            skill_packs: change,
            pinned: None,
        };
        round_trips(&command);
        assert!(command.validate().is_ok());
    }
}

#[test]
fn open_and_forget_name_only_the_matter() {
    let matter = matter_id(1);
    for command in [
        HostCommand::OpenMatter { matter },
        HostCommand::ForgetMatter { matter },
    ] {
        round_trips(&command);
        assert!(command.validate().is_ok());
    }
    let json = format!(r#"{{"forget_matter":{{"matter":"{matter}","delete":true}}}}"#);
    assert!(serde_json::from_str::<HostCommand>(&json).is_err());
}

#[test]
fn a_host_error_names_a_matter_only_when_given_one() {
    let plain = HostError::new(ErrorCode::MatterFolderIsHome, "home");
    assert_eq!(plain.matter(), None);
    let json = serde_json::to_string(&plain).unwrap();
    assert!(!json.contains("matter\":"), "{json}");
    let named = HostError::new(ErrorCode::MatterFolderIsMatter, "taken").with_matter(matter_id(7));
    let json = serde_json::to_string(&named).unwrap();
    assert!(
        json.contains(&format!(r#""matter":"{}""#, matter_id(7))),
        "{json}"
    );
    let back: HostError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, named);
    assert_eq!(back.matter(), Some(matter_id(7)));
}

#[test]
fn the_default_location_query_and_its_answer_have_the_apps_wire_form() {
    assert_eq!(
        serde_json::to_string(&HostQuery::GetDefaultLocation).unwrap(),
        r#""get_default_location""#
    );
    round_trips(&HostQuery::GetDefaultLocation);
    let answer = QueryResult::DefaultLocation(DefaultLocation {
        display: "Documents/Clauseo".to_owned(),
    });
    let json = r#"{"default_location":{"display":"Documents/Clauseo"}}"#;
    assert_eq!(serde_json::to_string(&answer).unwrap(), json);
    assert_eq!(serde_json::from_str::<QueryResult>(json).unwrap(), answer);
    let extra = r#"{"default_location":{"display":"x","path":"/x"}}"#;
    assert!(serde_json::from_str::<QueryResult>(extra).is_err());
}
