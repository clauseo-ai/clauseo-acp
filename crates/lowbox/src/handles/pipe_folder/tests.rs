//! The pipe folder's make, open and check over a fake NPFS: the squatted, tampered and missing
//! cases the VM does not plant.

use std::cell::RefCell;
use std::collections::HashMap;

use super::*;

const PACKAGE: &str = "S-1-15-2-4291742471-3177126929-1079639061-2360262239-4224205492-\
                       3771709147-1509459664";
/// The user who builds the namespace (the recipe's desktop user).
const OWNER: &str = "S-1-5-21-4086760546-1163544284-2580614524-1001";
/// The browser account `B_s` (the recipe's).
const BROWSER: &str = "S-1-5-21-4086760546-1163544284-2580614524-1002";
/// Another account: a pool user who could try to plant the folder first.
const POOL_USER: &str = "S-1-5-21-4086760546-1163544284-2580614524-1007";
/// The NPFS root's name, which a prefix handle's `NtQueryObject` name also reads as.
const ROOT: &str = r"\Device\NamedPipe";
/// `STATUS_OBJECT_NAME_NOT_FOUND`.
const NOT_FOUND: i32 = 0xC000_0034_u32.cast_signed();
/// `FILE_OPENED`: `IO_STATUS_BLOCK.Information` when `FILE_OPEN_IF` found the file there.
const FILE_OPENED: usize = 1;
/// `ERROR_INVALID_PARAMETER`: what reading the NPFS root's descriptor gave on the VM.
const INVALID_PARAMETER: u32 = 87;

/// A prefix's owner and security, as the fake keeps them.
#[derive(Clone)]
struct Prefix {
    owner: String,
    security: Security,
}

/// NPFS as the checks see it: prefixes by name (letters in either case), every name asked for,
/// and, for the fallback case, a kernel that answers a missing name with the root.
#[derive(Default)]
struct FakeNpfs {
    prefixes: RefCell<HashMap<String, Prefix>>,
    asked: RefCell<Vec<String>>,
    root_for_missing: bool,
}

/// The DACL and label parts of the SDDL `sddl` (`O:…D:…S:…`), as the real conversion writes
/// each on its own.
fn split(sddl: &str) -> Security {
    let d = sddl.find("D:").unwrap();
    let s = sddl.find("S:").unwrap_or(sddl.len());
    Security {
        dacl: sddl[d..s].to_owned(),
        label: sddl[s..].to_owned(),
    }
}

/// The owner (`O:`) of the SDDL `sddl`.
fn owner_of(sddl: &str) -> String {
    let start = sddl.strip_prefix("O:").unwrap();
    start[..start.find("D:").unwrap()].to_owned()
}

impl FakeNpfs {
    /// Someone made the folder `name` first, owned by `owner`, with `security`.
    fn plant(&self, name: &str, owner: &str, security: Security) {
        self.prefixes.borrow_mut().insert(
            name.to_lowercase(),
            Prefix {
                owner: owner.to_owned(),
                security,
            },
        );
    }

    fn prefix(&self, name: &str) -> Option<Prefix> {
        self.prefixes.borrow().get(&name.to_lowercase()).cloned()
    }
}

impl PipeFs for FakeNpfs {
    /// The name of the object opened: a prefix's, or [`ROOT`].
    type Folder = String;

    fn create(&self, name: &str, sddl: &str) -> Result<(String, usize), Error> {
        self.asked.borrow_mut().push(name.to_owned());
        if self.prefix(name).is_some() {
            // FILE_OPEN_IF on an existing prefix opens it; the descriptor given is ignored.
            return Ok((name.to_owned(), FILE_OPENED));
        }
        self.plant(name, &owner_of(sddl), split(sddl));
        Ok((name.to_owned(), FILE_CREATED))
    }

    fn open(&self, name: &str) -> Result<String, Error> {
        self.asked.borrow_mut().push(name.to_owned());
        match self.prefix(name) {
            Some(_) => Ok(name.to_owned()),
            None if self.root_for_missing => Ok(ROOT.to_owned()),
            None => Err(Error::Nt {
                call: "NtCreateFile",
                status: NOT_FOUND,
            }),
        }
    }

    fn owner(&self, folder: &String) -> Result<String, u32> {
        self.prefix(folder)
            .map(|p| p.owner)
            .ok_or(INVALID_PARAMETER)
    }

    fn security(&self, folder: &String) -> Result<Security, u32> {
        self.prefix(folder)
            .map(|p| p.security)
            .ok_or(INVALID_PARAMETER)
    }

    fn security_of(&self, sddl: &str) -> Result<Security, Error> {
        Ok(split(sddl))
    }
}

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// The folder's name in session 1.
fn name() -> String {
    layout::object_name(Slot::Pipes, 1, &sid(PACKAGE))
}

/// The DACL and label the owner sets.
fn f1() -> Security {
    split(&layout::pipe_folder_sddl(
        &sid(OWNER),
        &sid(BROWSER),
        &sid(PACKAGE),
    ))
}

/// The guard's open of the package's folder in session 1, as `B_s`, for the builder `OWNER`.
fn guard_open(fs: &FakeNpfs) -> Result<String, Error> {
    let (package, owner) = (sid(PACKAGE), sid(OWNER));
    let expected = Expected {
        package: &package,
        session: 1,
        owner: &owner,
    };
    open(fs, &expected, &sid(BROWSER))
}

/// The set-up's make of the package's folder in session 1, as `OWNER`.
fn set_up(fs: &FakeNpfs) -> Result<String, Error> {
    make(fs, 1, &sid(OWNER), &sid(BROWSER), &sid(PACKAGE))
}

fn refused(problem: HandleProblem) -> Result<String, Error> {
    Err(Error::Handle {
        slot: Slot::Pipes,
        problem,
    })
}

/// A folder planted by the builder itself (owner right) with `dacl` and F1's label.
fn with_dacl(dacl: String) -> FakeNpfs {
    let fs = FakeNpfs::default();
    fs.plant(
        &name(),
        OWNER,
        Security {
            dacl,
            label: f1().label,
        },
    );
    fs
}

/// The set-up makes the folder new, and the guard opens that one by its name and takes it.
#[test]
fn the_guard_takes_the_folder_set_up_made() {
    let fs = FakeNpfs::default();
    assert_eq!(set_up(&fs), Ok(name()));
    assert_eq!(guard_open(&fs), Ok(name()));
    assert_eq!(*fs.asked.borrow(), [name(), name()]);
}

/// Squat case 1: A folder of another owner (a pool user who planted it, `B_s`, Administrators), with F1's
/// DACL and label copied exactly, is refused; so is one whose owner cannot be read.
#[test]
fn the_guard_refuses_a_folder_of_another_owner() {
    for other in [POOL_USER, BROWSER, "S-1-5-32-544"] {
        let fs = FakeNpfs::default();
        fs.plant(&name(), other, f1());
        assert_eq!(
            guard_open(&fs),
            refused(HandleProblem::WrongOwner),
            "{other}"
        );
    }
    assert_eq!(
        check(Err(5), Ok(f1()), &expected_for_check(), &f1()),
        Err(Error::Handle {
            slot: Slot::Pipes,
            problem: HandleProblem::OwnerUnreadable { code: 5 }
        })
    );
}

/// Squat case 2: The right owner but a DACL of its own is refused: the recipe's untrimmed package mask, a
/// missing package ACE, a widened `B_s` ACE, a deny ACE in place of an allow.
#[test]
fn the_guard_refuses_the_right_owner_with_another_dacl() {
    let dacl = f1().dacl;
    let package_ace = format!("(A;;0x120006;;;{PACKAGE})");
    let browser_ace = format!("(A;;0x1201ff;;;{BROWSER})");
    assert!(dacl.contains(&package_ace) && dacl.contains(&browser_ace));
    for other in [
        dacl.replacen(&package_ace, &format!("(A;;0x1f0006;;;{PACKAGE})"), 1),
        dacl.replacen(&package_ace, "", 1),
        dacl.replacen(&browser_ace, &format!("(A;;FA;;;{BROWSER})"), 1),
        dacl.replacen(&package_ace, &format!("(D;;0x120006;;;{PACKAGE})"), 1),
    ] {
        assert_eq!(
            guard_open(&with_dacl(other.clone())),
            refused(HandleProblem::WrongDacl),
            "{other}"
        );
    }
}

/// Squat case 3: F1's ACEs plus one more, first or last, are refused.
#[test]
fn the_guard_refuses_a_folder_with_an_extra_ace() {
    let dacl = f1().dacl;
    for other in [
        format!("{dacl}(A;;FA;;;WD)"),
        dacl.replacen("D:", &format!("D:(A;;0x120006;;;{POOL_USER})"), 1),
        format!("{dacl}(A;;0x120006;;;{PACKAGE})"),
    ] {
        assert_eq!(
            guard_open(&with_dacl(other.clone())),
            refused(HandleProblem::WrongDacl),
            "{other}"
        );
    }
}

/// Squat case 4: F1's ACEs in another order are refused (order decides access once a deny is among them).
#[test]
fn the_guard_refuses_a_folder_with_its_aces_reordered() {
    let dacl = f1().dacl;
    let first = "(A;;FA;;;SY)";
    let last_moved = format!("D:{}{first}", dacl["D:".len()..].replacen(first, "", 1));
    let package_ace = format!("(A;;0x120006;;;{PACKAGE})");
    let package_first = format!(
        "D:{package_ace}{}",
        dacl["D:".len()..].replacen(&package_ace, "", 1)
    );
    for other in [last_moved, package_first] {
        assert_ne!(other, dacl);
        assert_eq!(other.len(), dacl.len());
        assert_eq!(
            guard_open(&with_dacl(other.clone())),
            refused(HandleProblem::WrongDacl),
            "{other}"
        );
    }
}

/// Squat case 5: A missing folder fails the open with its NTSTATUS, and the only name asked for is the
/// package's folder, never the NPFS root. Even a kernel that answered with the root would be
/// refused: the root's descriptor does not read (as on the VM), so its owner is unreadable.
#[test]
fn a_missing_folder_fails_the_open_and_never_opens_the_root() {
    let fs = FakeNpfs::default();
    assert_eq!(
        guard_open(&fs),
        Err(Error::Nt {
            call: "NtCreateFile",
            status: NOT_FOUND
        })
    );
    assert_eq!(*fs.asked.borrow(), [name()]);
    assert!(fs.asked.borrow().iter().all(|n| !n.eq_ignore_ascii_case(ROOT)
        && !n.eq_ignore_ascii_case(&format!(r"{ROOT}\"))));

    let falls_back = FakeNpfs {
        root_for_missing: true,
        ..FakeNpfs::default()
    };
    assert_eq!(
        guard_open(&falls_back),
        refused(HandleProblem::OwnerUnreadable {
            code: INVALID_PARAMETER
        })
    );
}

/// Squat case 6: The set-up refuses to build over a folder it did not make, whoever made it and whatever
/// its descriptor, even an exact copy of F1 under the builder's own SID, and leaves it as it was.
#[test]
fn set_up_refuses_to_build_over_a_folder_it_did_not_make() {
    for (owner, security) in [
        (POOL_USER, f1()),
        (OWNER, f1()),
        (
            POOL_USER,
            Security {
                dacl: "D:(A;;FA;;;WD)".to_owned(),
                label: String::new(),
            },
        ),
    ] {
        let fs = FakeNpfs::default();
        fs.plant(&name(), owner, security.clone());
        assert_eq!(
            set_up(&fs),
            Err(Error::Namespace {
                reason: "the pipe folder was already there"
            }),
            "{owner}"
        );
        let left = fs.prefix(&name()).unwrap();
        assert_eq!((left.owner.as_str(), left.security), (owner, security));
    }
}

/// The label must be F1's Low, no-write-up label exactly: none, Medium, Low without no-write-up,
/// or an extra policy are refused, after the owner and the DACL passed.
#[test]
fn the_guard_refuses_a_folder_with_another_label() {
    for label in [
        "",
        "S:",
        "S:NO_ACCESS_CONTROL",
        "S:(ML;;NW;;;ME)",
        "S:(ML;;;;;LW)",
        "S:(ML;;NWNR;;;LW)",
        "S:(ML;;NW;;;LW)(AU;SA;FA;;;WD)",
    ] {
        let fs = FakeNpfs::default();
        fs.plant(
            &name(),
            OWNER,
            Security {
                dacl: f1().dacl,
                label: label.to_owned(),
            },
        );
        assert_eq!(
            guard_open(&fs),
            refused(HandleProblem::WrongLabel),
            "{label}"
        );
    }
}

/// The set-up also refuses a folder it made whose DACL or label reads back otherwise.
#[test]
fn set_up_refuses_a_new_folder_that_reads_back_otherwise() {
    struct Rewrites(FakeNpfs, Security);
    impl PipeFs for Rewrites {
        type Folder = String;
        fn create(&self, name: &str, sddl: &str) -> Result<(String, usize), Error> {
            self.0.create(name, sddl)
        }
        fn open(&self, name: &str) -> Result<String, Error> {
            self.0.open(name)
        }
        fn owner(&self, folder: &String) -> Result<String, u32> {
            self.0.owner(folder)
        }
        fn security(&self, _folder: &String) -> Result<Security, u32> {
            Ok(self.1.clone())
        }
        fn security_of(&self, sddl: &str) -> Result<Security, Error> {
            self.0.security_of(sddl)
        }
    }
    for read in [
        Security {
            dacl: format!("{}(A;;FA;;;WD)", f1().dacl),
            label: f1().label,
        },
        Security {
            dacl: f1().dacl,
            label: String::new(),
        },
    ] {
        assert_eq!(
            make(
                &Rewrites(FakeNpfs::default(), read),
                1,
                &sid(OWNER),
                &sid(BROWSER),
                &sid(PACKAGE)
            ),
            Err(Error::Namespace {
                reason: "the new pipe folder's DACL or label is not the one it was made with"
            })
        );
    }
}

fn expected_for_check() -> Expected<'static> {
    static PACKAGE_SID: std::sync::LazyLock<Sid> = std::sync::LazyLock::new(|| sid(PACKAGE));
    static OWNER_SID: std::sync::LazyLock<Sid> = std::sync::LazyLock::new(|| sid(OWNER));
    Expected {
        package: &PACKAGE_SID,
        session: 1,
        owner: &OWNER_SID,
    }
}

/// The DACL and SACL control flags grant nothing and are not compared; unknown flags, an ACL
/// without ACEs, and text after the ACEs are never a match.
#[test]
fn control_flags_aside_the_acl_text_must_match_whole() {
    let wanted = f1();
    for flags in ["P", "AI", "PAI", "PARAI"] {
        let found = Security {
            dacl: wanted.dacl.replacen("D:", &format!("D:{flags}"), 1),
            label: wanted.label.replacen("S:", &format!("S:{flags}"), 1),
        };
        assert_eq!(
            check(
                Ok(OWNER.to_owned()),
                Ok(found),
                &expected_for_check(),
                &wanted
            ),
            Ok(()),
            "{flags}"
        );
    }
    for dacl in [
        "D:NO_ACCESS_CONTROL".to_owned(),
        "D:".to_owned(),
        String::new(),
        format!("{}S:(ML;;NW;;;LW)", wanted.dacl),
        wanted.dacl.replacen("D:", "D:X", 1),
        wanted.dacl.replacen("D:", "S:", 1),
    ] {
        let found = Security {
            dacl: dacl.clone(),
            label: wanted.label.clone(),
        };
        assert_eq!(
            check(
                Ok(OWNER.to_owned()),
                Ok(found),
                &expected_for_check(),
                &wanted
            ),
            Err(Error::Handle {
                slot: Slot::Pipes,
                problem: HandleProblem::WrongDacl
            }),
            "{dacl}"
        );
    }
    assert_eq!(
        check(Ok(OWNER.to_owned()), Err(1), &expected_for_check(), &wanted),
        Err(Error::Handle {
            slot: Slot::Pipes,
            problem: HandleProblem::SecurityUnreadable { code: 1 }
        })
    );
}
