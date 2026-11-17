//! The preflights' read-only handling (`CLAUSEO_PLACE_FREE`, `CLAUSEO_PLACE_RESTORE`,
//! `CLAUSEO_PLACE_FREED` in the template's `place-file` block): only the read-only bit is cleared,
//! only where it is set, and a stopped run puts it back on exactly those files, so it leaves every
//! attribute as it was. The copy loop keeps its full reset. Run by `nsis_run` on a fake machine.

use std::collections::HashMap;

use super::admin_copy::macro_body;
use super::nsis_run::{Machine, defines, parse_all, preprocess};
use super::place_files::{place_code, place_lines};

const A: &str = r"C:\Users\a\AppData\Local\Clauseo\Clauseo.exe";
const B: &str = r"C:\Users\a\AppData\Local\Clauseo\clauseo-mcp.exe";
const READONLY: i64 = 1;
const HIDDEN: i64 = 2;
const SYSTEM: i64 = 4;
const ARCHIVE: i64 = 32;

/// A preflight block over `A` then `B`, as the blocks insert it (the count reset, a
/// `CLAUSEO_PLACE_FREE` per file, `CLAUSEO_PLACE_FREED`), each file with its attributes, `B`
/// held or not, run with `vars`: the machine it leaves.
fn run_preflight(a: i64, b: i64, b_held: bool, vars: &[(&str, &str)]) -> Machine {
    let code = place_code();
    let mut defines: HashMap<String, String> = defines(&code);
    defines.insert("_CLAUSEO_FILE".to_owned(), "FILE".to_owned());
    let free = place_lines(&code, "CLAUSEO_PLACE_FREE", &defines);
    let for_file = |path: &str| -> Vec<String> {
        free.iter()
            .map(|line| line.replace("${_CLAUSEO_FILE}", path))
            .collect()
    };
    let mut lines = vec!["StrCpy $ClauseoPlaceCleared 0".to_owned()];
    lines.extend(for_file(A));
    lines.extend(for_file(B));
    lines.extend(preprocess(
        &macro_body(&code, "CLAUSEO_PLACE_FREED")[1..],
        &defines,
    ));
    lines.push("DetailPrint rest".to_owned());
    let mut machine = Machine::new(defines, None);
    for (var, value) in vars {
        machine.set_var(var, value);
    }
    machine.files.extend([A.to_owned(), B.to_owned()]);
    machine.attributes.insert(A.to_owned(), a);
    machine.attributes.insert(B.to_owned(), b);
    if b_held {
        machine.locked.push(B.to_owned());
    }
    machine.run(&parse_all(&lines));
    assert!(machine.stack.is_empty(), "{:?}", machine.stack);
    machine
}

/// The `SetFileAttributesW` lines `machine` said.
fn attribute_sets(machine: &Machine) -> Vec<&str> {
    machine
        .said
        .iter()
        .map(String::as_str)
        .filter(|line| line.starts_with("SetFileAttributesW "))
        .collect()
}

#[test]
fn the_preflight_clears_only_the_read_only_bit_and_only_where_it_is_set() {
    let machine = run_preflight(READONLY | HIDDEN, HIDDEN | ARCHIVE, false, &[]);
    assert!(!machine.aborted);
    // A's read-only bit off, its hidden bit kept; B, not read-only, untouched.
    assert_eq!(
        attribute_sets(&machine),
        [format!("SetFileAttributesW {A} {HIDDEN}")]
    );
    assert_eq!(machine.attributes[A], HIDDEN);
    assert_eq!(machine.attributes[B], HIDDEN | ARCHIVE);
    assert_eq!(machine.said.last().unwrap(), "DetailPrint rest");
    // The preflight's macro never resets attributes wholesale; the copy loop still does.
    let code = place_code();
    let free = macro_body(&code, "CLAUSEO_PLACE_FREE");
    assert!(
        free.iter()
            .all(|line| !line.starts_with("SetFileAttributes ") && !line.contains("NORMAL")),
        "{free:?}"
    );
    assert!(
        macro_body(&code, "CLAUSEO_PLACE_BEGIN")
            .contains(&"SetFileAttributes \"${_CLAUSEO_FILE}\" NORMAL".to_owned())
    );
}

#[test]
fn a_stopped_preflight_puts_back_every_read_only_bit_it_cleared() {
    let a = READONLY | HIDDEN;
    let b = READONLY | HIDDEN | SYSTEM;
    for (vars, end) in [
        // Section Install: 29 and Abort.
        (vec![], ["SetErrorLevel 29", "Abort"].as_slice()),
        // The reinstall page, interactive: back to the page, no code.
        (
            vec![("$ClauseoPlacePage", "1"), ("$PassiveMode", "0")],
            ["Abort"].as_slice(),
        ),
        // The reinstall page, passive: 29 and Quit (the fake goes on to the Abort NSIS never
        // reaches).
        (
            vec![("$ClauseoPlacePage", "1"), ("$PassiveMode", "1")],
            ["SetErrorLevel 29", "Quit", "Abort"].as_slice(),
        ),
    ] {
        let machine = run_preflight(a, b, true, &vars);
        assert!(machine.aborted, "{vars:?}");
        assert_eq!(machine.attributes[A], a, "{vars:?}");
        assert_eq!(machine.attributes[B], b, "{vars:?}");
        // Cleared on A, then B; put back on B, then A, after the box and before the stop.
        assert_eq!(
            attribute_sets(&machine),
            [
                format!("SetFileAttributesW {A} {}", a & !READONLY),
                format!("SetFileAttributesW {B} {}", b & !READONLY),
                format!("SetFileAttributesW {B} {b}"),
                format!("SetFileAttributesW {A} {a}"),
            ],
            "{vars:?}"
        );
        let shown = machine
            .said
            .iter()
            .position(|line| line == "MessageBox $(clauseoFileInUse)")
            .unwrap();
        let restored = machine
            .said
            .iter()
            .position(|line| *line == format!("SetFileAttributesW {A} {a}"))
            .unwrap();
        assert!(shown < restored);
        assert_eq!(
            machine.said[machine.said.len() - end.len()..],
            *end,
            "{vars:?}"
        );
    }
}
