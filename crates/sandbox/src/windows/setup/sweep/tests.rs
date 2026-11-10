//! `--sweep-tasks` through a fake machine, on every OS: the refusals touch nothing, only tasks
//! whose principal resolves to exactly a pool SID are deleted, in every folder however deep,
//! the pool's BITS jobs are cancelled, and every failure is counted without stopping the rest.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use super::*;
use crate::windows::setup::pool_steps::Pool;

mod exits;
mod file_owner;
mod orphans;
mod principals;
mod record;
mod running;
mod sequence;
mod task_file;
mod unlisted;
mod walk;

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
    /// The local accounts' names `LookupAccountSidW` gives, by SID.
    names: BTreeMap<Sid, String>,
    /// This computer's names.
    computer: Vec<String>,
    /// Task names whose delete fails.
    delete_refuse: Vec<String>,
    /// The running task instances: each its task's path, `Err` where it can't be read; `Err`
    /// when the list can't be.
    running: Result<Vec<Result<String, Failed>>, Failed>,
    /// Indexes of the instances whose stop fails.
    stop_refuse: Vec<usize>,
    /// Tasks the task files and the cache hold but the listing hides, by path; and whether
    /// each of those listings (the task files', the cache's) fails.
    hidden: Vec<String>,
    listings_fail: [bool; 2],
    /// The owners the cache's `SD` values give, by task path, `Err` where one is missing or
    /// can't be read; one not here is the Administrators'.
    cache_owners: BTreeMap<String, Result<Sid, Failed>>,
    /// Keys only the cache holds (no task file, not listed), by path; and whether the cache
    /// holds a task past each key, by path, `Err` where that can't be read; one not here does.
    cache_only: Vec<String>,
    cache_entries: BTreeMap<String, Result<bool, Failed>>,
    /// Each task's security descriptor by its name, `Err` where it can't be read; one not
    /// here is the Administrators'.
    owners: BTreeMap<String, Result<String, Failed>>,
    /// The task files by their parts below `Tasks`, joined by `\`; `Err` where one can't be
    /// read, and one not here is missing.
    files: BTreeMap<String, Result<Vec<u8>, Failed>>,
    /// The task files' owners and DACLs, by the same key; one not here is [`clean`].
    securities: BTreeMap<String, FileSecurity>,
    /// The BITS jobs' owners; `Err` when the list can't be read.
    jobs: Listed,
    /// Indexes of the jobs whose cancel fails.
    jobs_refuse: Vec<usize>,
    /// The record's `REG_QWORD` values (`SweepNext`, …), how often the set-up lock is held now
    /// (0 or 1), and whether it stays held by another.
    values: BTreeMap<&'static str, u64>,
    held: u8,
    busy: bool,
    /// The value whose write fails.
    write_fails: Option<&'static str>,
    /// The record's reads, and the read (0 the first) from which the key is gone.
    pool_reads: usize,
    gone_from: Option<usize>,
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
            names: BTreeMap::new(),
            // What GetComputerNameExW gives: the NetBIOS name, and the DNS host name, which here
            // is the same name and so is listed once.
            computer: vec!["PC-7".to_owned()],
            delete_refuse: Vec::new(),
            running: Ok(Vec::new()),
            stop_refuse: Vec::new(),
            hidden: Vec::new(),
            listings_fail: [false; 2],
            cache_owners: BTreeMap::new(),
            cache_only: Vec::new(),
            cache_entries: BTreeMap::new(),
            owners: BTreeMap::new(),
            files: BTreeMap::new(),
            securities: BTreeMap::new(),
            jobs: Ok(Vec::new()),
            jobs_refuse: Vec::new(),
            values: BTreeMap::new(),
            held: 0,
            busy: false,
            write_fails: None,
            pool_reads: 0,
            gone_from: None,
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

    /// Every task the machine holds, by path: those the folders list, and the hidden ones.
    fn on_disk(&self) -> Vec<String> {
        let mut paths = self.hidden.clone();
        for (folder, held) in &self.folders {
            let tasks = held.tasks.iter();
            paths.extend(tasks.map(|task| super::running::task_path(folder, &task.name)));
        }
        paths
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

/// A task file's security as the Task Scheduler leaves it: owned by Administrators, all access
/// for SYSTEM and Administrators, read and execute for Users.
fn clean() -> FileSecurity {
    let allow = |sid: &str, mask| (Some(Sid::parse(sid).unwrap()), mask);
    FileSecurity {
        owner: Sid::parse("S-1-5-32-544").unwrap(),
        allows: Some(vec![
            allow("S-1-5-18", 0x001F_01FF),
            allow("S-1-5-32-544", 0x001F_01FF),
            allow("S-1-5-32-545", 0x0012_00A9),
        ]),
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
        self.pool_reads += 1;
        if self.gone_from.is_some_and(|from| self.pool_reads > from) {
            return Ok(None);
        }
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

    fn task_sddl(&mut self, _folder: &String, name: &str) -> Result<String, Failed> {
        let sddl = self.owners.get(name).cloned();
        sddl.unwrap_or_else(|| Ok("O:BAG:SYD:(A;;FA;;;BA)(A;;FA;;;SY)".to_owned()))
    }

    fn account_name(&mut self, sid: &Sid) -> Result<String, Failed> {
        let found = self.names.get(sid).cloned();
        found.ok_or_else(|| "no local account has the SID".to_owned())
    }

    fn computer_names(&mut self) -> Vec<String> {
        self.computer.clone()
    }

    fn folder_path(&mut self, folder: &String) -> Result<String, Failed> {
        Ok(folder.clone())
    }

    fn read_task_file(&mut self, parts: &[String]) -> Result<TaskFile, Failed> {
        let path = parts.join("\\");
        self.done.push(format!("file {path}"));
        let file = self.files.get(&path).cloned();
        let bytes = file.unwrap_or_else(|| Err("the task file is missing".to_owned()))?;
        let security = self.securities.get(&path).cloned().unwrap_or_else(clean);
        Ok(TaskFile { bytes, security })
    }

    fn running_tasks(&mut self) -> Result<Vec<Result<String, Failed>>, Failed> {
        self.done.push("running".to_owned());
        self.running.clone()
    }

    fn stop_running(&mut self, at: usize) -> Result<(), Failed> {
        if self.stop_refuse.contains(&at) {
            return Err("IRunningTask::Stop failed (0x80070005)".to_owned());
        }
        let path = self.running.as_ref().unwrap()[at].clone().unwrap();
        self.done.push(format!("stop {path}"));
        Ok(())
    }

    fn same_task_path(&mut self, a: &str, b: &str) -> Result<bool, Failed> {
        // As CompareStringOrdinal ignores case: each character upper-cased alone, by its simple
        // mapping. The Kelvin sign is upper case already and stays itself.
        let upper = |text: &str| -> String {
            let one = |c: char| {
                let mut up = c.to_uppercase();
                match (up.next(), up.next()) {
                    (Some(single), None) => single,
                    _ => c,
                }
            };
            text.chars().map(one).collect()
        };
        Ok(upper(a) == upper(b))
    }

    fn task_file_paths(&mut self) -> Result<Vec<String>, Failed> {
        if self.listings_fail[0] {
            return Err("a folder below Tasks couldn't be listed (5)".to_owned());
        }
        Ok(self.on_disk())
    }

    fn task_cache_paths(&mut self) -> Result<Vec<String>, Failed> {
        if self.listings_fail[1] {
            return Err("a key of the Task Scheduler's cache couldn't be listed (5)".to_owned());
        }
        let mut paths = self.on_disk();
        paths.extend(self.cache_only.iter().cloned());
        Ok(paths)
    }

    fn task_cache_owner(&mut self, path: &str) -> Result<Sid, Failed> {
        let admins = || Ok(Sid::parse("S-1-5-32-544").unwrap());
        self.cache_owners.get(path).cloned().unwrap_or_else(admins)
    }

    fn task_cache_entries(&mut self, path: &str) -> Result<bool, Failed> {
        self.cache_entries.get(path).cloned().unwrap_or(Ok(true))
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
        "the sandbox's task sweep: 2 pool users, 1 folders and 12 tasks read, 3 of the pool's \
         tasks deleted, 0 unreadable (0 more skipped, not the pool's by their task file), 0 with \
         no user or group, 0 unresolved (no SID, not another domain's), 0 skipped (another \
         domain's, no SID), 0 not deleted, 0 not listed (in a task file or the cache only), 0 \
         orphan cache keys (no task, not the pool's), 0 running instances stopped, 0 not, 0 BITS jobs cancelled, 0 not"
    );
}

#[test]
fn the_service_names_map_to_their_sids_in_any_case_and_nothing_else_does() {
    let mut fake = Fake::new(&[1010]);
    let names = Names::default();
    for (name, want) in [
        ("LocalSystem", "S-1-5-18"),
        ("LOCALSERVICE", "S-1-5-19"),
        ("networkservice", "S-1-5-20"),
        ("S-1-5-21-1-2-3-1010", "S-1-5-21-1-2-3-1010"),
    ] {
        let sid = Sid::parse(want).unwrap();
        assert_eq!(resolve(&mut fake, &names, name), Resolved::Sid(sid));
    }
    assert_eq!(fake.lookups(), Vec::<&str>::new());
    // Anything else is looked up, a near spelling or a domain-qualified name too; with no
    // computer name known, none is another domain's.
    for name in ["Local System", r"NT AUTHORITY\LocalSystem", "SYSTEM"] {
        let found = resolve(&mut fake, &names, name);
        assert!(matches!(found, Resolved::Unresolved(_)), "{name}");
    }
    assert_eq!(fake.lookups().len(), 3);
}

#[test]
fn an_unreadable_principal_is_56_and_the_pools_other_tasks_still_go() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\", "first", user("S-1-5-21-1-2-3-1010"));
    fake.unread("\\", "unread");
    fake.unread("\\", "unread-too");
    fake.task("\\", "last", group("S-1-5-21-1-2-3-1010"));
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(SweepExit::TasksLeft.as_code(), 56);
    assert_eq!(fake.deleted(), ["\\first", "\\last"]);
    // The BITS step still ran, and then the pool tasks' instances were listed.
    let tail: Vec<&str> = fake.done.iter().rev().take(2).map(String::as_str).collect();
    assert_eq!(tail, ["running", "jobs"]);
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
