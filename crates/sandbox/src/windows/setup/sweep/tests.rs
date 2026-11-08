//! `--sweep-tasks` through a fake machine, on every OS: the refusals touch nothing, only tasks
//! whose principal resolves to exactly a pool SID are deleted, in every folder however deep,
//! the pool's BITS jobs are cancelled, and every failure is counted without stopping the rest.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use super::*;
use crate::windows::setup::pool_steps::Pool;

mod basics;
mod exits;
mod file_owner;
mod groups;
mod hardening;
mod log_path;
mod principals;
mod record;
mod running;
mod sequence;
mod swapped;
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
    /// Task files no cache key holds, and cache keys no task file holds, by path; the task
    /// files' own owners by their parts joined by `\`, `Err` where one can't be read and one
    /// not here the Administrators'.
    stray: Vec<String>,
    stale: Vec<String>,
    file_owners: BTreeMap<String, Result<Sid, Failed>>,
    /// Mid-run: tasks (folder, name, principal's `UserId`) made once the walk is done, as the task
    /// files are first listed; paths whose file only the first listing holds; and how many of
    /// the cache's listings, from the first, have a key vanish. The task files' listings read.
    late: Vec<(String, String, String)>,
    once: Vec<String>,
    vanish: usize,
    listings: usize,
    /// How many of the task files' listings, from the first, have a folder vanish; whether a
    /// folder or key gone is there again when checked; and whether each cache key's task can
    /// run, by path (one not here can).
    vanish_folder: usize,
    present_again: Result<bool, Failed>,
    runnable: BTreeMap<String, Result<bool, Failed>>,
    /// The owners the cache's `SD` values give, by task path, `Err` where one is missing or
    /// can't be read; one not here is the Administrators'.
    cache_owners: BTreeMap<String, Result<Sid, Failed>>,
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
    /// The record's `GroupSid`; and the local groups, each its members or why they can't be
    /// read (a SID here, or under `S-1-5-32`, or one of the well-known groups, is a group's).
    pool_group: Result<Option<Sid>, Failed>,
    groups: BTreeMap<Sid, Result<Vec<Sid>, Failed>>,
    done: Vec<String>,
    logged: Vec<(bool, String)>,
    /// What the admin-only log was given, and why it fails when `Some`.
    admin_logged: Vec<String>,
    admin_fails: Option<Failed>,
    /// Each task's whole descriptor by its name, as bytes, `Err` where it can't be read; one
    /// not here is [`hardening::admin_owned`]'s. The names whose descriptor write fails, and each
    /// write made (name, descriptor).
    descriptors: BTreeMap<String, Result<Vec<u8>, Failed>>,
    secure_refuse: Vec<String>,
    secured: Vec<(String, Vec<u8>)>,
    /// Each task's SDDL as the Task Scheduler gives it, by name; one not here is
    /// [`DEFAULT_SDDL`]. The names whose SACL can't be read, and those whose descriptor a write
    /// leaves as [`hardening::admin_owned`] (another owner, no SACL) whatever it was given.
    sddls: BTreeMap<String, String>,
    sacl_refuse: Vec<String>,
    secure_mangles: Vec<String>,
    /// The names whose write is recorded but leaves the descriptor as it was.
    secure_keeps: Vec<String>,
    /// SIDs whose kind (`is_group`) can't be read.
    kinds_unread: Vec<Sid>,
    /// The tasks' cache `Id`s and the record's `OwnerUntrusted` ([`swapped::Ids`]).
    ids: swapped::Ids,
}

/// The SDDL the fake gives a task not in `Fake::sddls`.
const DEFAULT_SDDL: &str = "O:BAG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;BU)";

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
            stray: Vec::new(),
            stale: Vec::new(),
            file_owners: BTreeMap::new(),
            late: Vec::new(),
            once: Vec::new(),
            vanish: 0,
            listings: 0,
            vanish_folder: 0,
            present_again: Ok(false),
            runnable: BTreeMap::new(),
            cache_owners: BTreeMap::new(),
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
            pool_group: Ok(None),
            groups: BTreeMap::new(),
            done: Vec::new(),
            logged: Vec::new(),
            admin_logged: Vec::new(),
            admin_fails: None,
            descriptors: BTreeMap::new(),
            secure_refuse: Vec::new(),
            secured: Vec::new(),
            sddls: BTreeMap::new(),
            sacl_refuse: Vec::new(),
            secure_mangles: Vec::new(),
            secure_keeps: Vec::new(),
            kinds_unread: Vec::new(),
            ids: swapped::Ids::default(),
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
        ..Principal::default()
    }
}

/// A principal naming `group`, and no user.
fn group(group: &str) -> Principal {
    Principal {
        group_id: group.to_owned(),
        ..Principal::default()
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

    fn task_file_paths(&mut self) -> Result<Listing, Failed> {
        for (folder, name, principal) in std::mem::take(&mut self.late) {
            self.task(&folder, &name, user(&principal));
        }
        self.listings += 1;
        if self.listings_fail[0] {
            return Err("a folder below Tasks couldn't be listed (5)".to_owned());
        }
        let mut paths = self.on_disk();
        paths.extend(self.stray.iter().cloned());
        if self.listings == 1 {
            paths.extend(self.once.iter().cloned());
        }
        let gone = self.listings <= self.vanish_folder;
        let vanished = if gone {
            vec![r"\Gone".to_owned()]
        } else {
            Vec::new()
        };
        Ok(Listing { paths, vanished })
    }

    fn task_cache_paths(&mut self) -> Result<Listing, Failed> {
        if self.listings_fail[1] {
            return Err("a key of the Task Scheduler's cache couldn't be listed (5)".to_owned());
        }
        let mut paths = self.on_disk();
        paths.extend(self.stale.iter().cloned());
        let gone = self.listings <= self.vanish;
        let vanished = if gone {
            vec![r"\A\gone".to_owned()]
        } else {
            Vec::new()
        };
        Ok(Listing { paths, vanished })
    }

    fn task_file_owner(&mut self, parts: &[String]) -> Result<Sid, Failed> {
        let admins = || Ok(Sid::parse("S-1-5-32-544").unwrap());
        let owner = self.file_owners.get(&parts.join("\\")).cloned();
        owner.unwrap_or_else(admins)
    }

    fn task_cache_runnable(&mut self, path: &str) -> Result<bool, Failed> {
        self.runnable.get(path).cloned().unwrap_or(Ok(true))
    }

    fn task_folder_present(&mut self, _path: &str) -> Result<bool, Failed> {
        self.present_again.clone()
    }

    fn task_cache_key_present(&mut self, _path: &str) -> Result<bool, Failed> {
        self.present_again.clone()
    }

    fn task_cache_owner(&mut self, path: &str) -> Result<Sid, Failed> {
        let admins = || Ok(Sid::parse("S-1-5-32-544").unwrap());
        self.cache_owners.get(path).cloned().unwrap_or_else(admins)
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

    fn admin_log(&mut self, line: &str) -> Result<(), Failed> {
        self.admin_logged.push(line.to_owned());
        self.admin_fails.clone().map_or(Ok(()), Err)
    }

    fn pool_group(&mut self) -> Result<Option<Sid>, Failed> {
        self.pool_group.clone()
    }

    fn is_group(&mut self, sid: &Sid) -> Result<bool, Failed> {
        if self.kinds_unread.contains(sid) {
            return Err("LookupAccountSidW failed (1332)".to_owned());
        }
        let text = sid.as_str();
        let known = text.starts_with("S-1-5-32-") || POOL_GROUPS.contains(&text);
        Ok(known || self.groups.contains_key(sid))
    }

    fn group_members(&mut self, sid: &Sid) -> Result<Vec<Sid>, Failed> {
        self.done.push(format!("members {}", sid.as_str()));
        let members = self.groups.get(sid).cloned();
        members.unwrap_or_else(|| Err("NetLocalGroupGetMembers failed (2220)".to_owned()))
    }

    fn task_security(
        &mut self,
        _folder: &String,
        name: &str,
        sacl: bool,
    ) -> Result<harden::TaskSecurity, Failed> {
        self.done.push(format!("security {name} {sacl}"));
        if sacl && self.sacl_refuse.iter().any(|refused| refused == name) {
            return Err("IRegisteredTask::GetSecurityDescriptor failed (0x80070522)".to_owned());
        }
        let found = self.descriptors.get(name).cloned();
        let mut bytes = found.unwrap_or_else(|| Ok(hardening::admin_owned()))?;
        if !sacl {
            hardening::drop_sacl(&mut bytes);
        }
        let sddl = self.sddls.get(name).cloned();
        let sddl = sddl.unwrap_or_else(|| DEFAULT_SDDL.to_owned());
        Ok(harden::TaskSecurity { bytes, sddl })
    }

    fn set_task_security(
        &mut self,
        _folder: &String,
        name: &str,
        descriptor: &[u8],
    ) -> Result<(), Failed> {
        if self.secure_refuse.iter().any(|refused| refused == name) {
            return Err("IRegisteredTask::SetSecurityDescriptor failed (0x80070005)".to_owned());
        }
        self.secured.push((name.to_owned(), descriptor.to_vec()));
        if self.secure_keeps.iter().any(|kept| kept == name) {
            return Ok(());
        }
        let stored = if self.secure_mangles.iter().any(|mangled| mangled == name) {
            hardening::admin_owned()
        } else {
            descriptor.to_vec()
        };
        self.descriptors.insert(name.to_owned(), Ok(stored));
        Ok(())
    }

    fn task_cache_id(&mut self, path: &str) -> Result<Option<String>, Failed> {
        swapped::cache_id(self, path)
    }

    fn owner_untrusted(&mut self) -> Result<Vec<String>, Failed> {
        Ok(self.ids.record.clone())
    }

    fn set_owner_untrusted(&mut self, entries: &[String]) -> Result<(), Failed> {
        self.ids.held.push(self.held == 1);
        if self.ids.store_fails {
            return Err("RegSetValueExW failed (5)".to_owned());
        }
        self.ids.record = entries.to_vec();
        Ok(())
    }
}
