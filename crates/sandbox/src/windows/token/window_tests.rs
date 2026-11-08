//! The agent's window station's and desktop's descriptors: as made, and as left for the run.

use super::*;

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

const POOL: &str = "S-1-5-21-1111111111-2222222222-3333333333-1005";
const LAWYER: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
const MATTER: &str = "S-1-5-21-444444444-555555555-666666666-1234";
/// `WRITE_DAC`, `WRITE_OWNER` and `DELETE`.
const TAKE_OVER: u32 = 0x0004_0000 | 0x0008_0000 | 0x0001_0000;

/// The DACL's entries, `(A;;0x<mask>;;;<sid>)` each, as (mask, SID); every one an allow with a
/// hex mask, or the test fails.
fn entries(sddl: &str) -> Vec<(u32, String)> {
    let dacl = sddl.strip_prefix("D:P").unwrap();
    let dacl = dacl.split("S:").next().unwrap();
    dacl.trim_start_matches('(')
        .trim_end_matches(')')
        .split(")(")
        .map(|entry| {
            let parts: Vec<&str> = entry.split(';').collect();
            let [kind, _, mask, _, _, sid] = <[&str; 6]>::try_from(parts).unwrap();
            assert_eq!(kind, "A", "{entry}");
            let mask = mask.strip_prefix("0x").unwrap();
            (u32::from_str_radix(mask, 16).unwrap(), sid.to_owned())
        })
        .collect()
}

/// The mask of `who`'s one entry.
fn mask_of(sddl: &str, who: &str) -> u32 {
    let found: Vec<u32> = entries(sddl)
        .into_iter()
        .filter(|(_, sid)| sid == who)
        .map(|(mask, _)| mask)
        .collect();
    assert_eq!(found.len(), 1, "{who} in {sddl}");
    found.first().copied().unwrap()
}

fn all_four() -> [String; 4] {
    let (lawyer, pool, matter) = (sid(LAWYER), sid(POOL), sid(MATTER));
    [
        station_creation_sddl(&lawyer),
        desktop_creation_sddl(&lawyer),
        station_sddl(&lawyer, &pool, Some(&matter)),
        desktop_sddl(&lawyer, &pool, Some(&matter)),
    ]
}

#[test]
fn the_station_is_made_shut_to_its_owner_with_the_lawyer_able_to_take_it_over() {
    assert_eq!(
        station_creation_sddl(&sid(LAWYER)),
        format!("D:P(A;;0xF037F;;;SY)(A;;0xE0008;;;{LAWYER})(A;;0x2007F;;;OW)")
    );
}

#[test]
fn the_desktop_is_made_with_the_lawyer_holding_what_create_desktop_asks_for() {
    assert_eq!(
        desktop_creation_sddl(&sid(LAWYER)),
        format!("D:P(A;;0xF01FF;;;SY)(A;;0xE00C7;;;{LAWYER})(A;;0xC7;;;OW)")
    );
}

#[test]
fn the_last_descriptors_are_the_verified_ones() {
    let (lawyer, pool) = (sid(LAWYER), sid(POOL));
    assert_eq!(
        station_sddl(&lawyer, &pool, None),
        format!(
            "D:P(A;;0xF037F;;;SY)(A;;0xE0000;;;{LAWYER})(A;;0x2007F;;;{POOL})\
             (A;;0x2007F;;;OW)S:(ML;;NW;;;LW)"
        )
    );
    assert_eq!(
        desktop_sddl(&lawyer, &pool, None),
        format!(
            "D:P(A;;0xF01FF;;;SY)(A;;0xE0000;;;{LAWYER})(A;;0xC7;;;{POOL})(A;;0xC7;;;OW)\
             S:(ML;;NW;;;LW)"
        )
    );
}

#[test]
fn the_matter_sid_gets_the_pool_users_rights_after_the_pool_user() {
    let (lawyer, pool, matter) = (sid(LAWYER), sid(POOL), sid(MATTER));
    assert_eq!(
        station_sddl(&lawyer, &pool, Some(&matter)),
        format!(
            "D:P(A;;0xF037F;;;SY)(A;;0xE0000;;;{LAWYER})(A;;0x2007F;;;{POOL})\
             (A;;0x2007F;;;{MATTER})(A;;0x2007F;;;OW)S:(ML;;NW;;;LW)"
        )
    );
    assert_eq!(
        desktop_sddl(&lawyer, &pool, Some(&matter)),
        format!(
            "D:P(A;;0xF01FF;;;SY)(A;;0xE0000;;;{LAWYER})(A;;0xC7;;;{POOL})(A;;0xC7;;;{MATTER})\
             (A;;0xC7;;;OW)S:(ML;;NW;;;LW)"
        )
    );
}

#[test]
fn both_last_descriptors_are_labelled_low_and_protected() {
    let (lawyer, pool) = (sid(LAWYER), sid(POOL));
    for sddl in [
        station_sddl(&lawyer, &pool, None),
        desktop_sddl(&lawyer, &pool, None),
    ] {
        assert!(sddl.starts_with("D:P("), "{sddl}");
        assert!(sddl.ends_with("S:(ML;;NW;;;LW)"), "{sddl}");
    }
}

#[test]
fn every_descriptor_holds_owner_rights_to_use_rights() {
    for sddl in all_four() {
        let owner = mask_of(&sddl, "OW");
        assert_eq!(owner & TAKE_OVER, 0, "{sddl}");
        assert_eq!(owner & 0xF000_0000, 0, "no generic right: {sddl}");
        assert!(sddl.starts_with("D:P("), "{sddl}");
    }
}

#[test]
fn no_pool_sid_and_no_owner_can_take_either_over() {
    for sddl in all_four() {
        for (mask, who) in entries(&sddl) {
            if who == "SY" || who == LAWYER {
                continue;
            }
            assert!(
                [POOL, MATTER, "OW"].contains(&who.as_str()),
                "{who} in {sddl}"
            );
            assert_eq!(mask & TAKE_OVER, 0, "{who} in {sddl}");
        }
    }
}

#[test]
fn the_pool_user_gets_exactly_the_rights_user32_needs() {
    let (lawyer, pool) = (sid(LAWYER), sid(POOL));
    assert_eq!(mask_of(&station_sddl(&lawyer, &pool, None), POOL), 0x2007F);
    assert_eq!(mask_of(&desktop_sddl(&lawyer, &pool, None), POOL), 0xC7);
}

#[test]
fn the_lawyer_keeps_only_the_owners_descriptor_rights() {
    let (lawyer, pool) = (sid(LAWYER), sid(POOL));
    for sddl in [
        station_sddl(&lawyer, &pool, None),
        desktop_sddl(&lawyer, &pool, None),
    ] {
        assert_eq!(mask_of(&sddl, LAWYER), 0xE0000, "{sddl}");
    }
}

#[test]
fn each_call_asks_for_no_more_than_its_creation_dacl_grants() {
    let lawyer = sid(LAWYER);
    let station = station_creation_sddl(&lawyer);
    let desktop = desktop_creation_sddl(&lawyer);
    assert_eq!(STATION_MAKE_RIGHTS & !mask_of(&station, "OW"), 0);
    assert_eq!(STATION_OPEN_RIGHTS & !mask_of(&station, LAWYER), 0);
    assert_eq!(DESKTOP_MAKE_RIGHTS & !mask_of(&desktop, LAWYER), 0);
    // CreateDesktopW asked for READ_CONTROL | WRITE_DAC | WRITE_OWNER alone fails with 8.
    assert_eq!(
        DESKTOP_MAKE_RIGHTS & DESKTOP_POOL_RIGHTS,
        DESKTOP_POOL_RIGHTS
    );
    // The station is opened to make the desktop on it.
    assert_eq!(STATION_OPEN_RIGHTS & 0x8, 0x8);
}

#[test]
fn no_descriptor_names_a_group_the_agent_holds() {
    for sddl in all_four() {
        for absent in [";WD)", ";BU)", ";RC)", ";AU)", "S-1-5-5-", ";IU)"] {
            assert!(!sddl.contains(absent), "{absent} in {sddl}");
        }
    }
}
