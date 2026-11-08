//! `--sweep-tasks` through a fake machine, on every OS: the refusals touch nothing, only tasks
//! whose principal resolves to exactly a pool SID are deleted, in every folder however deep,
//! the pool's BITS jobs are cancelled, and every failure is counted without stopping the rest.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use super::*;
use crate::windows::setup::pool_steps::Pool;

const OWNER: &str = "S-1-5-21-1-2-3-1001";
const ADMIN_EXE: &str = r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe";

fn sid(rid: u32) -> Sid {
    Sid::parse(&format!("S-1-5-21-1-2-3-{rid}")).unwrap()
}

/// A BITS list as the fake gives it: each job's owner, or why the list can't be read.
type Listed = Result<Vec<Option<Sid>>, Failed>;

/// A fake folder: its tasks and its subfolders, by path.
#[derive(Default)]
struct Folder {
    tasks: Vec<Task>,
    /// Each subfolder's path; `Err` where it can't be read.
    subs: Vec<Result<String, Failed>>,
    /// Its tasks can't be listed.
    tasks_fail: bool,
    /// Its subfolders can't be listed.
    subs_fail: bool,
}

/// The fake: the record, the folders, the accounts, the BITS jobs, and everything done.
struct Fake {
    system: bool,
    exe: PathBuf,
    pool: Result<Option<Pool>, Failed>,
    /// The pool-user check refuses.
    not_pool: bool,
    /// The folders by path; `\` the root.
    folders: BTreeMap<String, Folder>,
    /// The accounts `LookupAccountNameW` resolves.
    accounts: BTreeMap<String, Sid>,
    /// Task names whose delete fails.
    delete_refuse: Vec<String>,
    /// The BITS jobs' owners; `Err` when the list can't be read.
    jobs: Listed,
    /// Indexes of the jobs whose cancel fails.
    jobs_refuse: Vec<usize>,
    done: Vec<String>,
    logged: Vec<(bool, String)>,
}

impl Fake {
    fn new(users: &[u32]) -> Fake {
        let mut folders = BTreeMap::new();
        folders.insert("\\".to_owned(), Folder::default());
        Fake {
            system: true,
            exe: PathBuf::from(ADMIN_EXE),
            pool: Ok(Some(Pool {
                owner: Sid::parse(OWNER).unwrap(),
                users: users.iter().map(|rid| sid(*rid)).collect(),
            })),
            not_pool: false,
            folders,
            accounts: BTreeMap::new(),
            delete_refuse: Vec::new(),
            jobs: Ok(Vec::new()),
            jobs_refuse: Vec::new(),
            done: Vec::new(),
            logged: Vec::new(),
        }
    }

    /// Adds the task `name` with `principal` to the folder `path`, making the folder and its
    /// parents as needed.
    fn task(&mut self, path: &str, name: &str, principal: Principal) {
        self.folder(path).tasks.push(Task {
            name: name.to_owned(),
            principal: Ok(principal),
        });
    }

    /// Adds the task `name`, whose principal can't be read, to the folder `path`.
    fn unread(&mut self, path: &str, name: &str) {
        self.folder(path).tasks.push(Task {
            name: name.to_owned(),
            principal: Err("IRegisteredTask::Definition failed (0x80070005)".to_owned()),
        });
    }

    /// The folder `path`, made with its parents as needed.
    fn folder(&mut self, path: &str) -> &mut Folder {
        if path != "\\" && !self.folders.contains_key(path) {
            let (parent, _) = path.rsplit_once('\\').unwrap();
            let parent = if parent.is_empty() { "\\" } else { parent };
            self.folder(parent).subs.push(Ok(path.to_owned()));
        }
        self.folders.entry(path.to_owned()).or_default()
    }

    /// The task paths deleted, in order.
    fn deleted(&self) -> Vec<&str> {
        let deleted = self.done.iter().filter_map(|l| l.strip_prefix("delete "));
        deleted.collect()
    }

    /// The job indexes cancelled, in order.
    fn cancelled(&self) -> Vec<usize> {
        let at = self.done.iter().filter_map(|l| l.strip_prefix("cancel "));
        at.map(|at| at.parse().unwrap()).collect()
    }

    /// The names looked up, in order.
    fn lookups(&self) -> Vec<&str> {
        let names = self.done.iter().filter_map(|l| l.strip_prefix("lookup "));
        names.collect()
    }
}

/// A principal naming `user`, and no group.
fn user(user: &str) -> Principal {
    Principal {
        user_id: user.to_owned(),
        group_id: String::new(),
    }
}

/// A principal naming `group`, and no user.
fn group(group: &str) -> Principal {
    Principal {
        user_id: String::new(),
        group_id: group.to_owned(),
    }
}

impl Gate for Fake {
    fn is_system(&mut self) -> Result<bool, Failed> {
        Ok(self.system)
    }

    fn current_exe(&mut self) -> Result<PathBuf, Failed> {
        Ok(self.exe.clone())
    }

    fn admin_copy(&mut self) -> Result<PathBuf, Failed> {
        Ok(PathBuf::from(ADMIN_EXE))
    }

    fn pool(&mut self) -> Result<Option<Pool>, Failed> {
        self.pool.clone()
    }

    fn check_pool(&mut self, _pool: &Pool) -> Result<(), Failed> {
        if self.not_pool {
            return Err("a SID PoolSids lists failed the pool-user check".to_owned());
        }
        Ok(())
    }
}

impl Bits for Fake {
    fn bits_jobs(&mut self) -> Result<Vec<Option<Sid>>, Failed> {
        self.done.push("jobs".to_owned());
        self.jobs.clone()
    }

    fn cancel_job(&mut self, at: usize) -> Result<(), Failed> {
        if self.jobs_refuse.contains(&at) {
            return Err("IBackgroundCopyJob::Cancel failed (0x80070005)".to_owned());
        }
        self.done.push(format!("cancel {at}"));
        Ok(())
    }
}

impl Sweeper for Fake {
    type Folder = String;

    fn root(&mut self) -> Result<String, Failed> {
        self.done.push("root".to_owned());
        Ok("\\".to_owned())
    }

    fn subfolders(&mut self, folder: &String) -> Result<Vec<Result<String, Failed>>, Failed> {
        let folder = &self.folders[folder];
        if folder.subs_fail {
            return Err("ITaskFolder::GetFolders failed (0x80070005)".to_owned());
        }
        Ok(folder.subs.clone())
    }

    fn tasks(&mut self, folder: &String) -> Result<Vec<Task>, Failed> {
        let folder = &self.folders[folder];
        if folder.tasks_fail {
            return Err("ITaskFolder::GetTasks failed (0x80070005)".to_owned());
        }
        Ok(folder.tasks.clone())
    }

    fn lookup_account(&mut self, name: &str) -> Result<Sid, Failed> {
        self.done.push(format!("lookup {name}"));
        let found = self.accounts.get(name).cloned();
        found.ok_or_else(|| "LookupAccountNameW found no such account".to_owned())
    }

    fn delete_task(&mut self, folder: &String, name: &str) -> Result<(), Failed> {
        if self.delete_refuse.iter().any(|refused| refused == name) {
            return Err("ITaskFolder::DeleteTask failed (0x80070005)".to_owned());
        }
        let tasks = &mut self.folders.get_mut(folder).unwrap().tasks;
        tasks.retain(|task| task.name != name);
        let path = if folder == "\\" {
            format!("\\{name}")
        } else {
            format!("{folder}\\{name}")
        };
        self.done.push(format!("delete {path}"));
        Ok(())
    }

    fn log(&mut self, error: bool, line: &str) {
        self.logged.push((error, line.to_owned()));
    }
}

#[test]
fn only_tasks_whose_principal_is_exactly_a_pool_sid_are_deleted() {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.accounts.insert("ClauseoSb01".to_owned(), sid(1011));
    fake.accounts
        .insert("lawyer".to_owned(), Sid::parse(OWNER).unwrap());
    fake.accounts.insert("ClauseoSb1".to_owned(), sid(10111));
    // A pool SID by its text, and one by the account's name.
    fake.task("\\", "pool-sid", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\", "pool-name", user("ClauseoSb01"));
    // A SID a suffix or a prefix away from a pool SID, and a name that resolves to one.
    fake.task("\\", "longer", user("S-1-5-21-1-2-3-10101"));
    fake.task("\\", "shorter", user("S-1-5-21-1-2-3-101"));
    fake.task("\\", "near-name", user("ClauseoSb1"));
    // The lawyer's, SYSTEM's in each of its spellings, and the Users group's.
    fake.task("\\", "lawyer", user("lawyer"));
    fake.task("\\", "system", user("S-1-5-18"));
    fake.task("\\", "local-system", user("localsystem"));
    fake.task("\\", "network", user("NETWORKSERVICE"));
    fake.task("\\", "users", group("S-1-5-32-545"));
    // A group that is a pool SID; a user that isn't wins over a group that is.
    fake.task("\\", "pool-group", group("S-1-5-21-1-2-3-1011"));
    let both = Principal {
        user_id: "lawyer".to_owned(),
        group_id: "S-1-5-21-1-2-3-1010".to_owned(),
    };
    fake.task("\\", "user-first", both);
    fake.task("\\", "neither", Principal::default());
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(
        fake.deleted(),
        ["\\pool-sid", "\\pool-name", "\\pool-group"]
    );
    // A SID's text and the service names never reach the lookup; each name is looked up once.
    assert_eq!(fake.lookups(), ["ClauseoSb01", "ClauseoSb1", "lawyer"]);
    let (error, line) = &fake.logged[0];
    assert!(!error);
    assert_eq!(
        line,
        "the sandbox's task sweep: 2 pool users, 1 folders and 13 tasks read (1 with no user or \
         group), 3 of the pool's tasks deleted, 0 unreadable, 0 not deleted, 0 BITS jobs \
         cancelled, 0 not"
    );
}

#[test]
fn the_service_names_map_to_their_sids_in_any_case_and_nothing_else_does() {
    let mut fake = Fake::new(&[1010]);
    for (name, want) in [
        ("LocalSystem", "S-1-5-18"),
        ("LOCALSERVICE", "S-1-5-19"),
        ("networkservice", "S-1-5-20"),
        ("S-1-5-21-1-2-3-1010", "S-1-5-21-1-2-3-1010"),
    ] {
        assert_eq!(sid_of(&mut fake, name), Ok(Sid::parse(want).unwrap()));
    }
    assert_eq!(fake.lookups(), Vec::<&str>::new());
    // Anything else is looked up, a near spelling or a domain-qualified name too.
    for name in ["Local System", r"NT AUTHORITY\LocalSystem", "SYSTEM"] {
        assert!(sid_of(&mut fake, name).is_err(), "{name}");
    }
    assert_eq!(fake.lookups().len(), 3);
}

#[test]
fn an_unreadable_principal_is_54_and_the_pools_other_tasks_still_go() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\", "first", user("S-1-5-21-1-2-3-1010"));
    fake.unread("\\", "unread");
    fake.task("\\", "unknown-name", user("gone-user"));
    fake.task("\\", "last", group("S-1-5-21-1-2-3-1010"));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(SweepExit::TasksLeft.as_code(), 54);
    assert_eq!(fake.deleted(), ["\\first", "\\last"]);
    // The BITS step still ran.
    assert_eq!(fake.done.last().map(String::as_str), Some("jobs"));
    let (error, line) = &fake.logged[0];
    assert!(error);
    assert!(
        line.contains("2 of the pool's tasks deleted, 2 unreadable"),
        "{line}"
    );
}

#[test]
fn tasks_in_nested_subfolders_are_found_and_deleted() {
    let mut fake = Fake::new(&[1010]);
    let mut deep = String::new();
    for level in 0..40 {
        write!(deep, "\\F{level}").unwrap();
    }
    fake.task(&deep, "deepest", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\Microsoft\\Windows", "theirs", user("S-1-5-18"));
    fake.task("\\A\\B", "pool", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\A", "other", user("S-1-5-21-1-2-3-1013"));
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    let mut deleted = fake.deleted();
    deleted.sort_unstable();
    assert_eq!(deleted, ["\\A\\B\\pool", &format!("{deep}\\deepest")]);
    // The root, 40 deep, \Microsoft, \Microsoft\Windows, \A and \A\B.
    assert!(
        fake.logged[0].1.contains("45 folders and 4 tasks read"),
        "{:?}",
        fake.logged
    );
}

#[test]
fn a_folder_that_cant_be_listed_is_54_and_the_rest_are_still_walked() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\A", "hidden-from-us", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\A\\B", "below", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\C", "kept", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\C\\D", "under-c", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\E", "fine", user("S-1-5-21-1-2-3-1010"));
    // \A's tasks can't be listed, but its subfolders can; \C's subfolders can't, but its tasks
    // can; a subfolder of the root can't be read at all.
    fake.folder("\\A").tasks_fail = true;
    fake.folder("\\C").subs_fail = true;
    fake.folder("\\")
        .subs
        .push(Err("ITaskFolderCollection::Item failed".to_owned()));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    let mut deleted = fake.deleted();
    deleted.sort_unstable();
    assert_eq!(deleted, ["\\A\\B\\below", "\\C\\kept", "\\E\\fine"]);
    let line = &fake.logged[0].1;
    assert!(line.contains("; 3 folders couldn't be listed"), "{line}");
}

#[test]
fn a_failed_delete_is_54_and_the_task_is_counted() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\", "stuck", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\", "goes", user("S-1-5-21-1-2-3-1010"));
    fake.delete_refuse = vec!["stuck".to_owned()];
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.deleted(), ["\\goes"]);
    let line = &fake.logged[0].1;
    assert!(
        line.contains("1 of the pool's tasks deleted, 0 unreadable, 1 not deleted"),
        "{line}"
    );
}

#[test]
fn more_folders_than_the_cap_is_54_and_the_cap_itself_is_not() {
    for (extra, exit) in [(0, SweepExit::Done), (1, SweepExit::TasksLeft)] {
        let mut fake = Fake::new(&[1010]);
        // The root and FOLDERS_CAP - 1 below it, then `extra` more.
        for at in 0..FOLDERS_CAP - 1 + extra {
            fake.folder(&format!("\\F{at}"));
        }
        assert_eq!(sweep(&mut fake), exit, "{extra}");
        let line = &fake.logged[0].1;
        assert!(
            line.contains(&format!("{FOLDERS_CAP} folders and")),
            "{line}"
        );
        assert_eq!(
            line.contains("more than 10000 folders"),
            extra == 1,
            "{line}"
        );
    }
}

#[test]
fn only_the_pools_bits_jobs_are_cancelled_and_a_job_left_is_55() {
    let mut fake = Fake::new(&[1010, 1011]);
    let owner = Some(Sid::parse(OWNER).unwrap());
    fake.jobs = Ok(vec![
        owner,
        Some(sid(1010)),
        Some(sid(1013)),
        Some(sid(1011)),
    ]);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.cancelled(), [1, 3]);
    assert!(
        fake.logged[0].1.ends_with("2 BITS jobs cancelled, 0 not"),
        "{:?}",
        fake.logged
    );
    // A cancel that fails, an owner that can't be read, a list that can't be read or is cut.
    let cases: [(Listed, Vec<usize>); 4] = [
        (Ok(vec![Some(sid(1010)), Some(sid(1011))]), vec![0]),
        (Ok(vec![None, Some(sid(1011))]), Vec::new()),
        (
            Err("CoCreateInstance(BackgroundCopyManager) failed".to_owned()),
            Vec::new(),
        ),
        (Ok(vec![Some(sid(1013)); JOBS_CAP]), Vec::new()),
    ];
    for (jobs, refused) in cases {
        let mut fake = Fake::new(&[1010, 1011]);
        fake.jobs = jobs;
        fake.jobs_refuse = refused;
        assert_eq!(sweep(&mut fake), SweepExit::JobsLeft);
        assert!(fake.logged[0].0);
        assert_eq!(fake.deleted(), Vec::<&str>::new());
    }
    assert_eq!(SweepExit::JobsLeft.as_code(), 55);
    // A task left and a job left: 54 wins, and the job of the pool's is still cancelled.
    let mut fake = Fake::new(&[1010]);
    fake.unread("\\", "unread");
    fake.jobs = Ok(vec![None, Some(sid(1010))]);
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.cancelled(), [1]);
}

#[test]
fn the_refusals_touch_nothing_and_no_pool_is_nothing_to_sweep() {
    let refused = |fake: &mut Fake, exit: SweepExit, case: &str| {
        fake.task("\\", "pool", user("S-1-5-21-1-2-3-1010"));
        fake.jobs = Ok(vec![Some(sid(1010))]);
        assert_eq!(sweep(fake), exit, "{case}");
        assert!(fake.done.is_empty(), "{case}: {:?}", fake.done);
        assert!(fake.logged[0].0, "{case}");
    };
    let mut fake = Fake::new(&[1010]);
    fake.system = false;
    refused(&mut fake, SweepExit::NotAllowed, "not SYSTEM");
    let mut fake = Fake::new(&[1010]);
    fake.exe = PathBuf::from(r"C:\Users\lawyer\AppData\Local\Clauseo\clauseo-sandbox-setup.exe");
    refused(&mut fake, SweepExit::NotAllowed, "not the admin copy");
    let mut fake = Fake::new(&[1010]);
    fake.pool = Err("PoolSids is malformed".to_owned());
    refused(&mut fake, SweepExit::NoRecord, "no record");
    let mut fake = Fake::new(&[1010, 500]);
    fake.not_pool = true;
    refused(&mut fake, SweepExit::NotPool, "not a pool user");
    for pool in [None, Some(Vec::new())] {
        let mut fake = Fake::new(&[]);
        fake.pool = Ok(pool.map(|users| Pool {
            owner: Sid::parse(OWNER).unwrap(),
            users,
        }));
        fake.task("\\", "any", user("S-1-5-21-1-2-3-1010"));
        fake.jobs = Ok(vec![Some(sid(1010))]);
        assert_eq!(sweep(&mut fake), SweepExit::Done);
        assert!(fake.done.is_empty(), "{:?}", fake.done);
        assert!(!fake.logged[0].0);
    }
}

#[test]
fn the_log_line_holds_no_sid_task_name_or_account_name() {
    let mut fake = Fake::new(&[1010]);
    fake.accounts.insert("ClauseoSb01".to_owned(), sid(1010));
    fake.task("\\Secret", "MatterTask", user("ClauseoSb01"));
    fake.task("\\Secret", "Stuck", user("S-1-5-21-1-2-3-1010"));
    fake.task("\\Secret", "Unread", user("unknown-person"));
    fake.delete_refuse = vec!["Stuck".to_owned()];
    fake.jobs = Ok(vec![Some(sid(1010)), None]);
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    let line = &fake.logged[0].1;
    for secret in [
        "S-1-",
        "Secret",
        "MatterTask",
        "Stuck",
        "Unread",
        "ClauseoSb",
        "unknown",
    ] {
        assert!(!line.contains(secret), "{secret}: {line}");
    }
    // Counts only.
    assert!(
        line.contains("1 of the pool's tasks deleted, 1 unreadable, 1 not deleted"),
        "{line}"
    );
    assert!(line.contains("1 BITS jobs cancelled, 1 not"), "{line}");
}

#[test]
fn the_exit_codes_follow_the_stop_codes_and_are_never_shared() {
    use crate::windows::setup::stop::StopExit;
    let all = [
        SweepExit::Done,
        SweepExit::NotAllowed,
        SweepExit::NoRecord,
        SweepExit::NotPool,
        SweepExit::TasksLeft,
        SweepExit::JobsLeft,
    ];
    let codes: Vec<u8> = all.iter().map(|exit| exit.as_code()).collect();
    assert_eq!(codes, [0, 51, 52, 53, 54, 55]);
    let stop = StopExit::NotReserved.as_code();
    assert_eq!(stop, 50, "the stop's last code");
    let mut by_code = BTreeMap::new();
    for exit in all {
        assert!(by_code.insert(exit.as_code(), exit).is_none());
    }
    assert!(codes[1..].iter().all(|code| *code > stop));
}
