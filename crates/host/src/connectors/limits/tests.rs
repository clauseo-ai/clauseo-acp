//! The caps' counting, `day.json` and the local date.

use super::*;

/// A day count in a fresh folder of its own.
fn day_count(name: &str) -> (clauseo_connectors::fake::ScratchDir, DayCount) {
    let scratch = clauseo_connectors::fake::ScratchDir::new(name).unwrap();
    let path = scratch.path().join("connectors").join("day.json");
    (scratch, DayCount::load(path))
}

#[test]
fn spend_is_by_exact_tool_name_and_unknown_names_are_paid() {
    assert_eq!(Spend::of("account"), Spend::Free);
    assert_eq!(Spend::of("recharge"), Spend::Recharge);
    for paid in [
        "search_law",
        "read_document",
        "citator",
        "check_citations",
        "Account",
        "account ",
        "x",
    ] {
        assert_eq!(Spend::of(paid), Spend::Paid, "{paid}");
    }
}

#[test]
fn the_turn_and_between_turns_buckets() {
    let (_scratch, mut day) = day_count("buckets");
    let mut caps = TurnCaps::default();
    let today = "2026-11-02";
    for _ in 0..MAX_PAID_CALLS_PER_TURN {
        caps.take(Spend::Paid, &mut day, today).unwrap();
    }
    assert_eq!(caps.take(Spend::Paid, &mut day, today), Err(Cap::Turn));
    // A turn's end does not empty a between-turns bucket that is not the turn's.
    caps.end_turn();
    assert_eq!(caps.take(Spend::Paid, &mut day, today), Err(Cap::Turn));
    caps.start_turn();
    caps.take(Spend::Paid, &mut day, today).unwrap();
    caps.end_turn();
    // After a turn, a fresh between-turns bucket.
    for _ in 0..MAX_PAID_CALLS_PER_TURN {
        caps.take(Spend::Paid, &mut day, today).unwrap();
    }
    assert_eq!(caps.take(Spend::Paid, &mut day, today), Err(Cap::Turn));
    for _ in 0..MAX_RECHARGE_CALLS_PER_TURN {
        caps.take(Spend::Recharge, &mut day, today).unwrap();
    }
    assert_eq!(
        caps.take(Spend::Recharge, &mut day, today),
        Err(Cap::Recharge)
    );
    caps.take(Spend::Free, &mut day, today).unwrap();
    // A refused call is not counted against the day.
    assert_eq!(day.paid, 2 * MAX_PAID_CALLS_PER_TURN + 1);
}

#[test]
fn the_day_cap_resets_on_a_new_local_day_and_survives_a_restart() {
    let (scratch, mut day) = day_count("day");
    for _ in 0..MAX_PAID_CALLS_PER_DAY {
        day.take("2026-11-02").unwrap();
    }
    assert_eq!(day.take("2026-11-02"), Err(Cap::Day));
    let path = scratch.path().join("connectors").join("day.json");
    let mut again = DayCount::load(path.clone());
    assert_eq!(again.take("2026-11-02"), Err(Cap::Day));
    again.take("2026-11-03").unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        r#"{"day":"2026-11-03","paid":1}"#
    );
}

#[test]
fn a_day_file_that_does_not_parse_counts_from_0() {
    let scratch = clauseo_connectors::fake::ScratchDir::new("bad-day").unwrap();
    for text in [
        "not json",
        r#"{"day":"yesterday","paid":5}"#,
        r#"{"day":"2026-11-02","paid":-1}"#,
        r#"{"day":"2026-11-02","paid":1,"x":1}"#,
    ] {
        let path = scratch.write("day.json", text.as_bytes()).unwrap();
        let day = DayCount::load(path);
        assert_eq!((day.day.as_str(), day.paid), ("", 0), "{text}");
    }
}

#[test]
fn the_local_date_follows_the_offset() {
    // 2026-11-02 18:29:59 UTC is 23:59:59 in India; a second later it is the 3rd.
    let before = 1_793_644_199_000;
    assert_eq!(local_date(before, &FixedOffset(19_800)), "2026-11-02");
    assert_eq!(
        local_date(before + 1000, &FixedOffset(19_800)),
        "2026-11-03"
    );
    assert_eq!(local_date(before + 1000, &FixedOffset(0)), "2026-11-02");
    assert_eq!(local_date(0, &FixedOffset(0)), "1970-01-01");
    assert_eq!(local_date(-1, &FixedOffset(0)), "1969-12-31");
    // 2028-02-29 is a leap day; 2100-03-01 follows 2100-02-28.
    assert_eq!(local_date(1_835_395_200_000, &FixedOffset(0)), "2028-02-29");
    assert_eq!(local_date(4_107_542_400_000, &FixedOffset(0)), "2100-03-01");
    assert_eq!(local_date(4_107_542_399_000, &FixedOffset(0)), "2100-02-28");
}

#[test]
fn an_earlier_day_does_not_reset_the_count() {
    let (_scratch, mut day) = day_count("earlier");
    for _ in 0..MAX_PAID_CALLS_PER_DAY {
        day.take("2026-11-06").unwrap();
    }
    // A clock set back a day neither resets nor spends.
    assert_eq!(day.take("2026-11-05"), Err(Cap::Day));
    assert_eq!(day.take("2026-11-06"), Err(Cap::Day));
    day.take("2026-11-07").unwrap();
}

#[test]
fn a_day_far_ahead_does_not_hold_the_cap_once_the_clock_is_back() {
    let (_scratch, mut day) = day_count("future");
    // The clock once ran to 2030 and was set right.
    for _ in 0..MAX_PAID_CALLS_PER_DAY {
        day.take("2030-01-01").unwrap();
    }
    day.take("2026-11-07").unwrap();
    day.take("2026-11-14").unwrap();
    assert_eq!((day.day.as_str(), day.paid), ("2026-11-14", 1));
    // One day back (a time zone crossed) still counts against the day held.
    for _ in 1..MAX_PAID_CALLS_PER_DAY {
        day.take("2026-11-14").unwrap();
    }
    assert_eq!(day.take("2026-11-13"), Err(Cap::Day));
    // Across a month and a year end, one day back is still one day.
    let (_scratch, mut day) = day_count("year-end");
    day.take("2027-01-01").unwrap();
    day.take("2026-12-31").unwrap();
    assert_eq!((day.day.as_str(), day.paid), ("2027-01-01", 2));
    day.take("2026-12-30").unwrap();
    assert_eq!((day.day.as_str(), day.paid), ("2026-12-30", 1));
}
