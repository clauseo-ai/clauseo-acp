use std::collections::HashMap;

use super::*;

const SID: &str = "S-1-15-2-4291742471-3177126929-1079639061-2360262239-4224205492-3771709147-\
                   1509459664";

/// A process's handle table, as the checks see it.
#[derive(Default)]
struct Table {
    /// Value to (type, name).
    entries: HashMap<usize, (&'static str, String)>,
}

impl Table {
    /// The six objects a good owner hands over, at 0x10, 0x14, … 0x24.
    fn good() -> Table {
        let dir = format!(r"\Sessions\1\AppContainerNamedObjects\{SID}");
        let mut table = Table::default();
        let rows = [
            ("Directory", dir.clone()),
            ("Directory", format!(r"{dir}\RPC Control")),
            ("Directory", format!(r"{dir}\Global")),
            ("SymbolicLink", format!(r"{dir}\Local")),
            ("SymbolicLink", format!(r"{dir}\Session")),
            (
                "File",
                format!(r"\Device\NamedPipe\Sessions\1\AppContainerNamedObjects\{SID}"),
            ),
        ];
        for (i, row) in rows.into_iter().enumerate() {
            table.entries.insert(0x10 + 4 * i, row);
        }
        table
    }
}

const VALUES: [usize; 6] = [0x10, 0x14, 0x18, 0x1c, 0x20, 0x24];

impl Probe for Table {
    fn check_open(&self, value: usize) -> Result<(), u32> {
        // ERROR_INVALID_HANDLE.
        self.entries.get(&value).map(|_| ()).ok_or(6)
    }
    fn type_name(&self, value: usize) -> Result<String, i32> {
        Ok(self.entries[&value].0.to_owned())
    }
    fn object_name(&self, value: usize) -> Result<String, i32> {
        assert_ne!(
            self.entries[&value].0, "File",
            "the pipe folder's name is never asked"
        );
        Ok(self.entries[&value].1.clone())
    }
}

#[test]
fn the_arg_round_trips() {
    let text = format_handle_arg(&VALUES);
    assert_eq!(text, "0x10,0x14,0x18,0x1c,0x20,0x24");
    assert_eq!(parse_handle_arg(&text).unwrap(), VALUES);
    assert_eq!(
        parse_handle_arg("0xFFFFFFFF,0x1,0x2,0x3,0x4,0xAbC").unwrap(),
        [0xFFFF_FFFF, 1, 2, 3, 4, 0xABC]
    );
}

#[test]
fn a_bad_arg_is_refused() {
    for bad in [
        "",
        "0x10,0x14,0x18,0x1c,0x20",
        "0x10,0x14,0x18,0x1c,0x20,0x24,0x28",
        "0x10,0x14,0x18,0x1c,0x20,24",
        "0x10,0x14,0x18,0x1c,0x20,0x",
        "0x10,0x14,0x18,0x1c,0x20,0xg",
        "0x10,0x14,0x18,0x1c,0x20,0x-1",
        "0x10,0x14,0x18,0x1c,0x20,0x+1",
        "0x10,0x14,0x18,0x1c,0x20, 0x24",
        "0x10,0x14,0x18,0x1c,0x20,0x24,",
        "0x10,0x14,0x18,0x1c,0x20,0x10",
        "0x10,0x14,0x18,0x1c,0x20,0x0",
        "0x10,0x14,0x18,0x1c,0x20,0x00000000000000001",
    ] {
        assert!(
            matches!(parse_handle_arg(bad), Err(Error::HandleList { .. })),
            "{bad:?}"
        );
    }
}

#[test]
fn the_good_handles_pass() {
    assert_eq!(check(&Table::good(), &VALUES, SID), Ok(()));
}

#[test]
fn a_session_of_any_number_and_any_letter_case_passes() {
    let mut table = Table::good();
    for entry in table.entries.values_mut() {
        entry.1 = entry
            .1
            .replace(r"\Sessions\1\", r"\SESSIONS\12\")
            .to_lowercase();
    }
    assert_eq!(check(&table, &VALUES, SID), Ok(()));
}

#[test]
fn a_closed_value_fails_closed() {
    let mut table = Table::good();
    table.entries.remove(&0x1c);
    assert_eq!(
        check(&table, &VALUES, SID),
        Err(Error::Handle {
            slot: Slot::Local,
            problem: HandleProblem::NotOpen { code: 6 }
        })
    );
}

#[test]
fn a_wrong_type_fails_closed() {
    let mut table = Table::good();
    table.entries.get_mut(&0x24).unwrap().0 = "Directory";
    assert_eq!(
        check(&table, &VALUES, SID),
        Err(Error::Handle {
            slot: Slot::Pipes,
            problem: HandleProblem::WrongType {
                found: "Directory".to_owned()
            }
        })
    );
    let mut table = Table::good();
    table.entries.get_mut(&0x10).unwrap().0 = "Event";
    assert!(matches!(
        check(&table, &VALUES, SID),
        Err(Error::Handle {
            slot: Slot::Directory,
            problem: HandleProblem::WrongType { .. }
        })
    ));
}

#[test]
fn handles_out_of_order_fail_closed() {
    let mut table = Table::good();
    let global = table.entries.remove(&0x18).unwrap();
    let rpc = table.entries.insert(0x14, global).unwrap();
    table.entries.insert(0x18, rpc);
    assert_eq!(
        check(&table, &VALUES, SID),
        Err(Error::Handle {
            slot: Slot::RpcControl,
            problem: HandleProblem::WrongName
        })
    );
}

#[test]
fn another_package_or_place_fails_closed() {
    let other = SID.replace("1509459664", "1509459665");
    assert!(matches!(
        check(&Table::good(), &VALUES, &other),
        Err(Error::Handle {
            slot: Slot::Directory,
            problem: HandleProblem::WrongName
        })
    ));
    for dir in [
        format!(r"\Sessions\AppContainerNamedObjects\{SID}"),
        format!(r"\Sessions\x\AppContainerNamedObjects\{SID}"),
        format!(r"\BaseNamedObjects\{SID}"),
        format!(r"\Sessions\1\AppContainerNamedObjects\{SID}\Global"),
    ] {
        let mut table = Table::good();
        table.entries.get_mut(&0x10).unwrap().1 = dir.clone();
        assert!(
            matches!(
                check(&table, &VALUES, SID),
                Err(Error::Handle {
                    slot: Slot::Directory,
                    problem: HandleProblem::WrongName
                })
            ),
            "{dir}"
        );
    }
}

#[test]
fn a_link_in_another_session_fails_closed() {
    let mut table = Table::good();
    let session = &mut table.entries.get_mut(&0x20).unwrap().1;
    *session = session.replace(r"\Sessions\1\", r"\Sessions\2\");
    assert_eq!(
        check(&table, &VALUES, SID),
        Err(Error::Handle {
            slot: Slot::Session,
            problem: HandleProblem::WrongName
        })
    );
}

#[test]
fn a_repeated_value_fails_before_any_probe() {
    let values = [0x10, 0x10, 0x18, 0x1c, 0x20, 0x24];
    assert!(matches!(
        check(&Table::default(), &values, SID),
        Err(Error::HandleList { .. })
    ));
    assert!(matches!(
        check(&Table::default(), &VALUES[..5], SID),
        Err(Error::HandleList { .. })
    ));
}

#[test]
fn the_slots_say_their_types() {
    let kinds: Vec<&str> = Slot::ALL.iter().map(|s| s.kind().type_name()).collect();
    assert_eq!(
        kinds,
        [
            "Directory",
            "Directory",
            "Directory",
            "SymbolicLink",
            "SymbolicLink",
            "File"
        ]
    );
}
