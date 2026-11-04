use super::{Stat, parse_stat};

#[test]
fn stat_fields_are_read_after_the_last_parenthesis() {
    let line = "4242 (a (weird) name) S 17 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 \
                987654 1000 50 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0";
    assert_eq!(
        parse_stat(line),
        Some(Stat {
            parent: 17,
            group: 4242,
            start: 987_654,
            exited: false,
        })
    );
}

#[test]
fn a_zombie_has_exited_and_a_short_line_is_refused() {
    let zombie = "9 (sh) Z 1 9 9 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 55 0 0";
    assert!(parse_stat(zombie).is_some_and(|stat| stat.exited));
    assert_eq!(parse_stat("9 (sh) S 1 9"), None);
    assert_eq!(parse_stat("no parenthesis at all"), None);
}
