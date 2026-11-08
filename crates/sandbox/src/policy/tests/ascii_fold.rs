//! [`ascii_fold`](crate::policy::ascii_fold) against Unicode 16's CaseFolding.txt (the excerpt in
//! `case_folding_ascii.txt`) and against every code point's full fold by `caseless` (R18
//! MEDIUM-1).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;

use caseless::Caseless;

use crate::REFUSED_IN_OTHER_MATTER;
use crate::policy::ascii_fold::{BEYOND_ASCII, ascii_letter, folds_to, spellings};

const EXCERPT: &str = include_str!("../case_folding_ascii.txt");

/// The excerpt's lines as (code point, status, mapping).
fn lines() -> Vec<(char, char, Vec<char>)> {
    let hex = |s: &str| char::from_u32(u32::from_str_radix(s, 16).unwrap()).unwrap();
    EXCERPT
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split(';').map(str::trim).collect();
            let status = fields[1].chars().next().unwrap();
            (
                hex(fields[0]),
                status,
                fields[2].split(' ').map(hex).collect(),
            )
        })
        .collect()
}

#[test]
fn the_table_is_every_code_point_the_excerpt_folds_to_an_ascii_letter() {
    let lines = lines();
    assert!(EXCERPT.contains("CaseFolding-16.0.0.txt"));
    // Each letter's spellings from the excerpt: the letter, and every code point linked to it by
    // a C, S or T line, either way round (`0049; T; 0131` links ı to I, so to i).
    let mut want: BTreeMap<char, BTreeSet<char>> = ('a'..='z')
        .map(|l| (l, BTreeSet::from([l, l.to_ascii_uppercase()])))
        .collect();
    for (from, status, to) in &lines {
        if !matches!(status, 'C' | 'S' | 'T') {
            continue;
        }
        assert_eq!(to.len(), 1, "{from:?}");
        let letter = [*from, to[0]]
            .into_iter()
            .find(char::is_ascii_alphabetic)
            .unwrap()
            .to_ascii_lowercase();
        let set = want.get_mut(&letter).unwrap();
        set.insert(*from);
        set.insert(to[0]);
    }
    for (letter, set) in &want {
        let got: BTreeSet<char> = spellings(*letter).into_iter().collect();
        assert_eq!(&got, set, "{letter}");
        for c in set {
            assert_eq!(ascii_letter(*c), Some(*letter), "{c:?}");
        }
    }
    let beyond: BTreeSet<char> = want
        .values()
        .flatten()
        .filter(|c| !c.is_ascii())
        .copied()
        .collect();
    let table: BTreeSet<char> = BEYOND_ASCII.iter().map(|(from, _)| *from).collect();
    assert_eq!(table, beyond);
}

#[test]
fn every_code_point_caseless_folds_to_one_ascii_letter_is_in_the_table() {
    for c in (0..=0x10_FFFF).filter_map(char::from_u32) {
        let folded: String = std::iter::once(c).default_case_fold().collect();
        let mut chars = folded.chars();
        if let (Some(one), None) = (chars.next(), chars.next())
            && one.is_ascii_alphabetic()
        {
            assert_eq!(ascii_letter(c), Some(one), "{c:?}");
        } else if !matches!(c, '\u{130}' | '\u{131}') {
            // Only the Turkic two fold elsewhere and are kept anyway.
            assert_eq!(ascii_letter(c), None, "{c:?}");
        }
    }
}

#[test]
fn no_full_fold_spells_a_run_of_a_refused_name() {
    let folds: Vec<String> = lines()
        .into_iter()
        .filter(|(_, status, _)| *status == 'F')
        .map(|(_, _, to)| to.into_iter().collect())
        .collect();
    assert!(folds.contains(&"ss".to_owned()) && folds.contains(&"st".to_owned()));
    for name in REFUSED_IN_OTHER_MATTER {
        for fold in &folds {
            assert!(!name.contains(fold.as_str()), "{name} holds {fold}");
        }
    }
}

#[test]
fn a_name_folds_to_a_refused_name_in_any_case_and_with_a_long_s() {
    for (name, refused) in [
        (".clauseo", ".clauseo"),
        (".CLAUSEO", ".clauseo"),
        (".ClAuSeO", ".clauseo"),
        (".clau\u{17F}eo", ".clauseo"),
        (".agent\u{17F}", ".agents"),
        (".GIT", ".git"),
        (".g\u{131}t", ".git"),
        (".G\u{130}T", ".git"),
    ] {
        assert!(folds_to(OsStr::new(name), refused), "{name}");
    }
    for (name, refused) in [
        (".clauseox", ".clauseo"),
        (".clause", ".clauseo"),
        ("_clauseo", ".clauseo"),
        (".gi\u{1E97}", ".git"),
        (".github", ".git"),
    ] {
        assert!(!folds_to(OsStr::new(name), refused), "{name}");
    }
}
