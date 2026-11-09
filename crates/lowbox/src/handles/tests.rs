use std::collections::HashMap;

use super::*;

const SID: &str = "S-1-15-2-4291742471-3177126929-1079639061-2360262239-4224205492-3771709147-\
                   1509459664";
/// The user who built the namespace (the recipe's desktop user).
const OWNER: &str = "S-1-5-21-4086760546-1163544284-2580614524-1001";

/// A process's handle table, as the checks see it.
#[derive(Default)]
struct Table {
    /// Value to (type, name, owner).
    entries: HashMap<usize, (&'static str, String, String)>,
    /// The standard handles.
    std: [usize; 3],
}

impl Table {
    /// The five objects a good owner hands over, at 0x10, 0x14, … 0x20, in session 1.
    fn good() -> Table {
        let dir = format!(r"\Sessions\1\AppContainerNamedObjects\{SID}");
        let mut table = Table {
            std: [0x4, 0x8, 0xc],
            ..Table::default()
        };
        let rows = [
            ("Directory", dir.clone()),
            ("Directory", format!(r"{dir}\RPC Control")),
            ("Directory", format!(r"{dir}\Global")),
            ("SymbolicLink", format!(r"{dir}\Local")),
            ("SymbolicLink", format!(r"{dir}\Session")),
        ];
        for (i, (kind, name)) in rows.into_iter().enumerate() {
            table
                .entries
                .insert(0x10 + 4 * i, (kind, name, OWNER.to_owned()));
        }
        table
    }
}

/// The slot `result` refused for its name, if that was the refusal.
fn refused_for_its_name(result: &Result<(), Error>) -> Option<Slot> {
    match result {
        Err(Error::Handle {
            slot,
            problem: HandleProblem::WrongName { .. },
        }) => Some(*slot),
        _ => None,
    }
}

const VALUES: [usize; 5] = [0x10, 0x14, 0x18, 0x1c, 0x20];

impl Probe for Table {
    fn std_handles(&self) -> [usize; 3] {
        self.std
    }
    fn check_open(&self, value: usize) -> Result<(), u32> {
        // ERROR_INVALID_HANDLE.
        self.entries.get(&value).map(|_| ()).ok_or(6)
    }
    fn type_name(&self, value: usize) -> Result<String, i32> {
        Ok(self.entries[&value].0.to_owned())
    }
    fn object_name(&self, value: usize) -> Result<String, i32> {
        Ok(self.entries[&value].1.clone())
    }
    fn owner(&self, value: usize) -> Result<String, u32> {
        Ok(self.entries[&value].2.clone())
    }
}

/// The check's expectations: package B01 in session `session`, built by `OWNER`.
fn run(table: &Table, values: &[usize], session: u32) -> Result<(), Error> {
    let (package, owner) = (Sid::parse(SID).unwrap(), Sid::parse(OWNER).unwrap());
    let expected = Expected {
        package: &package,
        session,
        owner: &owner,
    };
    check(table, values, &expected)
}

#[test]
fn the_arg_round_trips() {
    let text = format_handle_arg(&VALUES);
    assert_eq!(text, "0x10,0x14,0x18,0x1c,0x20");
    assert_eq!(parse_handle_arg(&text).unwrap(), VALUES);
    assert_eq!(
        parse_handle_arg("0xFFFFFFFF,0x1,0x2,0x3,0xAbC").unwrap(),
        [0xFFFF_FFFF, 1, 2, 3, 0xABC]
    );
}

#[test]
fn a_bad_arg_is_refused() {
    for bad in [
        "",
        "0x10,0x14,0x18,0x1c",
        "0x10,0x14,0x18,0x1c,0x20,0x24",
        "0x10,0x14,0x18,0x1c,24",
        "0x10,0x14,0x18,0x1c,0x",
        "0x10,0x14,0x18,0x1c,0xg",
        "0x10,0x14,0x18,0x1c,0x-1",
        "0x10,0x14,0x18,0x1c,0x+1",
        "0x10,0x14,0x18,0x1c, 0x24",
        "0x10,0x14,0x18,0x1c,0x24,",
        "0x10,0x14,0x18,0x1c,0x10",
        "0x10,0x14,0x18,0x1c,0x0",
        "0x10,0x14,0x18,0x1c,0x00000000000000001",
    ] {
        assert!(
            matches!(parse_handle_arg(bad), Err(Error::HandleList { .. })),
            "{bad:?}"
        );
    }
}

#[test]
fn the_good_handles_pass() {
    assert_eq!(run(&Table::good(), &VALUES, 1), Ok(()));
}

#[test]
fn names_in_any_letter_case_pass() {
    let mut table = Table::good();
    for entry in table.entries.values_mut() {
        entry.1 = entry.1.replace(r"\Sessions\", r"\SESSIONS\").to_lowercase();
    }
    assert_eq!(run(&table, &VALUES, 1), Ok(()));
}

/// The namespace must be in the guard's own session: the same objects in session 12 fail when
/// the guard runs in session 1, and pass when it runs in 12.
#[test]
fn a_namespace_in_another_session_fails_closed() {
    let mut table = Table::good();
    for entry in table.entries.values_mut() {
        entry.1 = entry.1.replace(r"\Sessions\1\", r"\Sessions\12\");
    }
    assert_eq!(
        refused_for_its_name(&run(&table, &VALUES, 1)),
        Some(Slot::Directory)
    );
    assert_eq!(run(&table, &VALUES, 12), Ok(()));
    let mut zeros = Table::good();
    zeros.entries.get_mut(&0x10).unwrap().1 =
        format!(r"\Sessions\01\AppContainerNamedObjects\{SID}");
    assert!(run(&zeros, &VALUES, 1).is_err());
}

/// The pipe folder is never handed over: a sixth value (the old layout's pipe folder) is
/// refused before any probe, whatever it is.
#[test]
fn a_sixth_value_is_refused() {
    let mut table = Table::good();
    table.entries.insert(
        0x24,
        (
            "File",
            format!(r"\Device\NamedPipe\Sessions\1\AppContainerNamedObjects\{SID}"),
            OWNER.to_owned(),
        ),
    );
    assert_eq!(
        run(&table, &[0x10, 0x14, 0x18, 0x1c, 0x20, 0x24], 1),
        Err(Error::HandleList {
            reason: "not exactly five values"
        })
    );
}

/// A name refusal shows both names, with the package SID (in either case) and any other
/// package's SID written as placeholders; the refusal itself is the same.
#[test]
fn a_name_refusal_shows_both_names_without_a_package_sid() {
    let mut table = Table::good();
    table.entries.get_mut(&0x20).unwrap().1 = format!(
        r"\Sessions\1\AppContainerNamedObjects\{}\Session\",
        SID.to_lowercase()
    );
    let expected = r"\Sessions\1\AppContainerNamedObjects\<package SID>\Session";
    assert_eq!(
        run(&table, &VALUES, 1),
        Err(Error::Handle {
            slot: Slot::Session,
            problem: HandleProblem::WrongName {
                found: format!(r"{expected}\"),
                expected: expected.to_owned(),
            }
        })
    );
    let package = Sid::parse(SID).unwrap();
    for (name, shown) in [
        (
            r"\Device\NamedPipe\".to_owned(),
            r"\Device\NamedPipe\".to_owned(),
        ),
        (
            format!(r"Sessions\1\AppContainerNamedObjects\{SID}-"),
            r"Sessions\1\AppContainerNamedObjects\<package SID>-".to_owned(),
        ),
        (
            r"\x\S-1-15-2-1-2-3\y\s-1-15-2-".to_owned(),
            r"\x\<another package SID>\y\<another package SID>".to_owned(),
        ),
        (String::new(), String::new()),
    ] {
        assert_eq!(redact(&name, &package), shown, "{name}");
    }
}

/// Every object must be owned by the user who built the namespace: the guard's own user, or
/// anyone else, fails.
#[test]
fn an_object_of_another_owner_fails_closed() {
    for value in VALUES {
        let mut table = Table::good();
        table.entries.get_mut(&value).unwrap().2 =
            "S-1-5-21-4086760546-1163544284-2580614524-1002".to_owned();
        let refused = run(&table, &VALUES, 1);
        assert!(
            matches!(
                refused,
                Err(Error::Handle {
                    problem: HandleProblem::WrongOwner,
                    ..
                })
            ),
            "{value:#x}: {refused:?}"
        );
    }
}

/// A value equal to one of the guard's standard handles is refused before it is probed, even
/// when the object behind it would pass every other check.
#[test]
fn a_standard_handle_is_never_taken() {
    for std in 0..3 {
        let mut table = Table::good();
        table.std[std] = 0x20;
        assert_eq!(
            run(&table, &VALUES, 1),
            Err(Error::Handle {
                slot: Slot::Session,
                problem: HandleProblem::StandardHandle
            })
        );
    }
}

#[test]
fn a_closed_value_fails_closed() {
    let mut table = Table::good();
    table.entries.remove(&0x1c);
    assert_eq!(
        run(&table, &VALUES, 1),
        Err(Error::Handle {
            slot: Slot::Local,
            problem: HandleProblem::NotOpen { code: 6 }
        })
    );
}

#[test]
fn a_wrong_type_fails_closed() {
    let mut table = Table::good();
    table.entries.get_mut(&0x20).unwrap().0 = "File";
    assert_eq!(
        run(&table, &VALUES, 1),
        Err(Error::Handle {
            slot: Slot::Session,
            problem: HandleProblem::WrongType {
                found: "File".to_owned()
            }
        })
    );
    let mut table = Table::good();
    table.entries.get_mut(&0x10).unwrap().0 = "Event";
    assert!(matches!(
        run(&table, &VALUES, 1),
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
        refused_for_its_name(&run(&table, &VALUES, 1)),
        Some(Slot::RpcControl)
    );
}

#[test]
fn another_package_or_place_fails_closed() {
    let mut other = Table::good();
    for entry in other.entries.values_mut() {
        entry.1 = entry.1.replace("1509459664", "1509459665");
    }
    assert!(matches!(
        run(&other, &VALUES, 1),
        Err(Error::Handle {
            slot: Slot::Directory,
            problem: HandleProblem::WrongName { .. },
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
                run(&table, &VALUES, 1),
                Err(Error::Handle {
                    slot: Slot::Directory,
                    problem: HandleProblem::WrongName { .. },
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
        refused_for_its_name(&run(&table, &VALUES, 1)),
        Some(Slot::Session)
    );
}

#[test]
fn a_repeated_value_fails_before_any_probe() {
    let values = [0x10, 0x10, 0x18, 0x1c, 0x20];
    assert!(matches!(
        run(&Table::default(), &values, 1),
        Err(Error::HandleList { .. })
    ));
    assert!(matches!(
        run(&Table::default(), &VALUES[..4], 1),
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

#[test]
fn the_inherit_list_refuses_null_pseudo_and_repeated_handles() {
    assert_eq!(check_inherit_list(&[]), Ok(()));
    assert_eq!(check_inherit_list(&[0x10, 0x14]), Ok(()));
    for bad in [
        vec![0x10, 0],
        vec![usize::MAX],
        vec![usize::MAX - 1],
        vec![0x10, 0x14, 0x10],
    ] {
        assert!(
            matches!(check_inherit_list(&bad), Err(Error::InheritList { .. })),
            "{bad:x?}"
        );
    }
}

/// `--lowbox-handles` carries handle values and nothing else: no SID, no name, no path. The
/// guard re-derives the package SID and every name from the package name it is given apart.
#[test]
fn the_arg_carries_only_handle_values() {
    let values = [0x10, 0x14, 0x1_0000, 0x7fff_fffc, usize::MAX >> 4];
    let text = format_handle_arg(&values);
    assert!(
        text.split(',').all(|v| v.starts_with("0x")
            && v.len() > 2
            && v[2..].bytes().all(|b| b.is_ascii_hexdigit())),
        "{text}"
    );
    assert_eq!(text.split(',').count(), HANDED_COUNT);
    assert!(!text.contains("S-1-") && !text.contains('\\'));
    assert_eq!(parse_handle_arg(&text).unwrap(), values);
}

/// The browser account of the pipe folder tests (the recipe's).
const BROWSER: &str = "S-1-5-21-4086760546-1163544284-2580614524-1002";

/// F1's DACL for the test SIDs, as SDDL (`D:…`): the part of [`layout::pipe_folder_sddl`] the
/// owner sets and the guard compares.
fn f1_dacl() -> String {
    let sddl = layout::pipe_folder_sddl(
        &Sid::parse(OWNER).unwrap(),
        &Sid::parse(BROWSER).unwrap(),
        &Sid::parse(SID).unwrap(),
    );
    let start = sddl.find("D:").unwrap();
    let end = sddl.find("S:").unwrap();
    sddl[start..end].to_owned()
}

/// The pipe folder check on the guard's own handle: `owner` and `dacl` as read back from it.
fn pipe_check(owner: Result<&str, u32>, dacl: Result<String, u32>) -> Result<(), Error> {
    let (package, builder) = (Sid::parse(SID).unwrap(), Sid::parse(OWNER).unwrap());
    let expected = Expected {
        package: &package,
        session: 1,
        owner: &builder,
    };
    check_pipe_folder(owner.map(str::to_owned), dacl, &expected, &f1_dacl())
}

fn pipe_refusal(problem: HandleProblem) -> Result<(), Error> {
    Err(Error::Handle {
        slot: Slot::Pipes,
        problem,
    })
}

/// The folder the owner made passes: its owner and exactly F1's ACEs, with or without DACL
/// control flags (which grant nothing).
#[test]
fn the_owners_pipe_folder_passes() {
    assert_eq!(pipe_check(Ok(OWNER), Ok(f1_dacl())), Ok(()));
    for flags in ["P", "AI", "PAI", "PARAI"] {
        let flagged = f1_dacl().replacen("D:", &format!("D:{flags}"), 1);
        assert_eq!(pipe_check(Ok(OWNER), Ok(flagged)), Ok(()), "{flags}");
    }
}

/// A folder of another owner (the browser account, a squatter, Administrators) is refused, as
/// is one whose owner cannot be read; the owner is checked before the DACL.
#[test]
fn a_pipe_folder_of_another_owner_is_refused() {
    for other in [BROWSER, "S-1-5-32-544", "S-1-5-18"] {
        assert_eq!(
            pipe_check(Ok(other), Ok(f1_dacl())),
            pipe_refusal(HandleProblem::WrongOwner),
            "{other}"
        );
    }
    assert_eq!(
        pipe_check(Err(5), Ok(f1_dacl())),
        pipe_refusal(HandleProblem::OwnerUnreadable { code: 5 })
    );
}

/// Any DACL but exactly F1's ACEs in F1's order is refused: an extra ACE, a missing or widened
/// package ACE, a deny ACE, the ACEs reordered, no DACL, an empty one, or one with more after it.
#[test]
fn a_pipe_folder_with_another_dacl_is_refused() {
    let f1 = f1_dacl();
    let package_ace = format!("(A;;0x1f0006;;;{SID})");
    assert!(f1.contains(&package_ace));
    let first_ace = "(A;;FA;;;SY)";
    let reordered = {
        let rest = f1.replacen(first_ace, "", 1);
        format!("{rest}{first_ace}")
    };
    for dacl in [
        format!("{f1}(A;;FA;;;WD)"),
        f1.replacen("D:", "D:(A;;FA;;;S-1-5-11)", 1),
        f1.replacen(&package_ace, "", 1),
        f1.replacen(&package_ace, &format!("(A;;FA;;;{SID})"), 1),
        f1.replacen("D:", "D:(D;;FA;;;S-1-5-11)", 1),
        reordered,
        "D:NO_ACCESS_CONTROL".to_owned(),
        "D:".to_owned(),
        String::new(),
        format!("{f1}S:(ML;;NW;;;LW)"),
        f1.replacen("D:", "D:X", 1),
    ] {
        assert_eq!(
            pipe_check(Ok(OWNER), Ok(dacl.clone())),
            pipe_refusal(HandleProblem::WrongDacl),
            "{dacl}"
        );
    }
    assert_eq!(
        pipe_check(Ok(OWNER), Err(1)),
        pipe_refusal(HandleProblem::DaclUnreadable { code: 1 })
    );
}
