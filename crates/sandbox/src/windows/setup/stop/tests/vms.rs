//! The stop's WSL step (step 5b, `vms.rs`) through the fake: a VM whose folder the lawyer's or
//! another non-pool profile holds is left alone (a pool user holding it too is marked), a VM
//! whose folder a pool user's profile holds is ended and its user marked, a VM in no profile is
//! ended with every pool user marked unless a non-pool profile couldn't be read (then it is
//! unmapped: 48, nothing ended), a VM that isn't ended is 48, a list that fails is 48 only with
//! WSL installed, and with no HCS nothing is asked.

use super::*;

/// The lawyer's VM: the lawyer's profile holds its folder.
const LAWYER_VM: &str = "0F0E0D0C-0B0A-4908-8706-050403020100";
/// A pool user's VM, as WSL names its folder (upper case, no braces).
const POOL_VM: &str = "6B29FC40-CA47-1067-B31D-00DD010662DA";

/// HCS on the fake machine, and the pool's VM folders.
pub(super) struct FakeVms {
    /// `Stopper::compute_systems` (WSL's systems only): `Ok(None)` is no HCS.
    pub(super) systems: Result<Option<Vec<String>>, Failed>,
    /// `WSLService` or `LxssManager` is installed.
    pub(super) wsl: bool,
    /// The folders `AppData\Local\temp\<id>` each pool user's profile holds.
    pub(super) folders: Vec<(Sid, String)>,
    /// Pool users whose profile can't be read.
    pub(super) unreadable: Vec<Sid>,
    /// The folders `temp\<id>` the lawyer's `Local AppData` holds.
    pub(super) owner_folders: Vec<String>,
    /// The lawyer's profile, or its redirect, can't be read.
    pub(super) owner_unreadable: bool,
    /// The lawyer's folder reads.
    pub(super) owner_reads: usize,
    /// `ProfileList`'s SIDs; `None` is the lawyer's and the pool's.
    pub(super) profiles: Option<Result<Vec<Sid>, Failed>>,
    /// The folders `temp\<id>` other non-pool profiles' `Local AppData` hold.
    pub(super) other_folders: Vec<(Sid, String)>,
    /// Other non-pool profiles that can't be read.
    pub(super) other_unreadable: Vec<Sid>,
    /// Ids whose termination fails.
    pub(super) refuse: Vec<String>,
    /// Ids that never end: each termination waits out its wait, on the fake clock.
    pub(super) hang: Vec<String>,
    /// The folder reads, as (user's rid, id).
    pub(super) reads: usize,
    /// Pool users whose `AppData\Local\temp` holds a GUID-named folder (evidence (a)).
    pub(super) temp_guids: Vec<Sid>,
    /// Pool users whose loaded hive lists a WSL distribution (evidence (c)).
    pub(super) lxss: Vec<Sid>,
    /// HCS calls: lists and terminations.
    pub(super) hcs_calls: usize,
    /// The snapshots taken when each evidence read (a) or (c) was made.
    pub(super) evidence_reads: Vec<usize>,
}

impl Default for FakeVms {
    fn default() -> FakeVms {
        FakeVms {
            systems: Ok(None),
            wsl: true,
            folders: Vec::new(),
            unreadable: Vec::new(),
            owner_folders: Vec::new(),
            owner_unreadable: false,
            owner_reads: 0,
            profiles: None,
            other_folders: Vec::new(),
            other_unreadable: Vec::new(),
            refuse: Vec::new(),
            hang: Vec::new(),
            reads: 0,
            temp_guids: Vec::new(),
            lxss: Vec::new(),
            hcs_calls: 0,
            evidence_reads: Vec::new(),
        }
    }
}

impl VmStopper for Fake {
    fn compute_systems(&mut self) -> Result<Option<Vec<String>>, Failed> {
        self.vms.hcs_calls += 1;
        self.vms.systems.clone()
    }

    fn temp_vm_folder(&mut self, user: &Sid) -> Result<bool, Failed> {
        self.vms.evidence_reads.push(self.taken);
        if self.vms.unreadable.contains(user) {
            return Err("a pool user's temp folder couldn't be listed (5)".to_owned());
        }
        Ok(self.vms.temp_guids.contains(user))
    }

    fn lxss_registered(&mut self, user: &Sid) -> Result<bool, Failed> {
        self.vms.evidence_reads.push(self.taken);
        Ok(self.vms.lxss.contains(user))
    }

    fn wsl_installed(&mut self) -> bool {
        self.vms.wsl
    }

    fn vm_folder(&mut self, user: &Sid, id: &VmId) -> Result<bool, Failed> {
        self.vms.reads += 1;
        if self.vms.unreadable.contains(user) {
            return Err("a pool user's ProfileList entry couldn't be read".to_owned());
        }
        let folder = (user.clone(), id.as_str().to_owned());
        Ok(self.vms.folders.contains(&folder))
    }

    fn non_pool_vm_folder(&mut self, user: &Sid, id: &VmId) -> Result<bool, Failed> {
        let pool = self.pool.clone().ok().flatten().map(|pool| pool.users);
        let pool = pool.unwrap_or_default();
        assert!(!pool.contains(user), "no pool user's folder is read here");
        if user.as_str() != OWNER {
            if self.vms.other_unreadable.contains(user) {
                return Err("a non-pool profile couldn't be read (5)".to_owned());
            }
            let folder = (user.clone(), id.as_str().to_owned());
            return Ok(self.vms.other_folders.contains(&folder));
        }
        self.vms.owner_reads += 1;
        if self.vms.owner_unreadable {
            return Err("the lawyer's Local AppData redirect can't be expanded".to_owned());
        }
        Ok(self
            .vms
            .owner_folders
            .iter()
            .any(|held| held == id.as_str()))
    }

    fn profile_sids(&mut self) -> Result<Vec<Sid>, Failed> {
        let pool = self.pool.clone().ok().flatten().map(|pool| pool.users);
        let listed = std::iter::once(Sid::parse(OWNER).unwrap()).chain(pool.unwrap_or_default());
        self.vms
            .profiles
            .clone()
            .unwrap_or_else(|| Ok(listed.collect()))
    }

    fn terminate_vm(&mut self, id: &VmId, wait: Duration) -> Result<(), Failed> {
        self.vms.hcs_calls += 1;
        self.waits.push(wait);
        let id = id.as_str().to_owned();
        if self.vms.hang.contains(&id) {
            self.now += wait;
            return Err("HcsWaitForOperationResult (the termination) failed".to_owned());
        }
        if self.vms.refuse.contains(&id) {
            return Err("HcsTerminateComputeSystem failed (0x80070005)".to_owned());
        }
        self.done.push(format!("end vm {id}"));
        Ok(())
    }
}

/// A stop of pool users 1010 and 1011, with no process running, over HCS's `systems`, where
/// 1011's profile holds `POOL_VM`'s folder and the lawyer's `LAWYER_VM`'s.
fn with_vms(systems: &[&str]) -> Fake {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![Ok(vec![seen(11, Some(Sid::parse(OWNER).unwrap()))])];
    let systems = systems.iter().map(|id| (*id).to_owned()).collect();
    fake.vms.systems = Ok(Some(systems));
    fake.vms.folders = vec![(sid(1011), POOL_VM.to_owned())];
    fake.vms.owner_folders = vec![LAWYER_VM.to_owned()];
    // 1011's VM folder is evidence that a pool user started WSL.
    fake.vms.temp_guids = vec![sid(1011)];
    fake
}

/// The VMs `done` ended, in order.
fn ended(fake: &Fake) -> Vec<&str> {
    let ids = fake
        .done
        .iter()
        .filter_map(|line| line.strip_prefix("end vm "));
    ids.collect()
}

#[test]
fn a_pool_users_vm_is_ended_and_its_user_marked_and_the_lawyers_left() {
    // HCS's id for the pool's VM in another case and with braces.
    let braced = format!("{{{}}}", POOL_VM.to_ascii_lowercase());
    let mut fake = with_vms(&[LAWYER_VM, &braced]);
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(ended(&fake), [POOL_VM]);
    assert_eq!(fake.waits, [VM_WAIT]);
    assert_eq!(fake.resets, [sid(1011)]);
    // The lawyer first for each; each pool user for the lawyer's VM (a planted copy), then 1010
    // and 1011 for the pool's.
    assert_eq!(fake.vms.owner_reads, 2);
    assert_eq!(fake.vms.reads, 4);
    let (error, line) = &fake.logged[0];
    assert!(!error, "{line}");
    assert!(
        line.contains("0 not, 1 pool WSL VMs ended, 0 not, 0 unmapped, 0 in no profile; 1 users"),
        "{line}"
    );
    assert!(
        !line.contains(POOL_VM) && !line.contains("S-1-5"),
        "counts only: {line}"
    );
    assert!(fake.completed().is_some());
}

#[test]
fn a_pool_user_is_marked_before_its_vm_is_terminated() {
    let mut fake = with_vms(&[POOL_VM]);
    assert_eq!(stop(&mut fake), StopExit::Done);
    let at = |line: &str| fake.done.iter().position(|done| done == line);
    let (marked, ended) = (at("mark 1"), at(&format!("end vm {POOL_VM}")));
    assert!(marked.is_some() && marked < ended, "{:?}", fake.done);
    // A termination that hangs past the task's limit still leaves the user marked.
    let mut fake = with_vms(&[POOL_VM]);
    fake.vms.hang = vec![POOL_VM.to_owned()];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(fake.resets, [sid(1011)]);
    assert_eq!(
        fake.done
            .iter()
            .filter(|done| done.starts_with("mark"))
            .count(),
        1
    );
}

#[test]
fn with_no_hcs_nothing_is_asked_and_the_line_says_nothing_of_vms() {
    let mut fake = Fake::new(&[1010]);
    fake.vms.wsl = true;
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.vms.reads, 0);
    assert_eq!(ended(&fake), Vec::<&str>::new());
    assert!(!fake.logged[0].1.contains("VMs"), "{:?}", fake.logged);
}

#[test]
fn a_pool_vm_that_isnt_ended_is_48_and_its_user_still_marked() {
    let mut fake = with_vms(&[POOL_VM]);
    fake.vms.refuse = vec![POOL_VM.to_owned()];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(fake.resets, [sid(1011)]);
    assert_eq!(fake.completed(), None);
    let (error, line) = &fake.logged[0];
    assert!(error);
    assert!(line.contains("0 pool WSL VMs ended, 1 not"), "{line}");
    assert!(line.contains("WSL VM wasn't ended"), "{line}");
}

#[test]
fn the_terminations_stop_once_their_time_is_spent() {
    let ids = [POOL_VM, "6B29FC40-CA47-1067-B31D-00DD010662DB", LAWYER_VM];
    let mut fake = with_vms(&ids);
    fake.vms.owner_folders.clear();
    fake.vms.folders = ids.iter().map(|id| (sid(1010), (*id).to_owned())).collect();
    fake.vms.hang = ids.iter().map(|id| (*id).to_owned()).collect();
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    // 5 s each for two, then the 10 s are spent: the third isn't tried.
    assert_eq!(fake.waits, [VM_WAIT, VMS_BUDGET.saturating_sub(VM_WAIT)]);
    assert!(
        fake.logged[0].1.contains("0 pool WSL VMs ended, 3 not"),
        "{:?}",
        fake.logged
    );
}

#[test]
fn the_lawyers_vm_is_left_and_a_pool_users_planted_copy_of_its_folder_marks_that_user() {
    let mut fake = with_vms(&[LAWYER_VM]);
    fake.vms.folders = vec![(sid(1010), LAWYER_VM.to_owned())];
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(ended(&fake), Vec::<&str>::new());
    assert_eq!(fake.resets, [sid(1010)]);
    let line = &fake.logged[0].1;
    assert!(
        line.contains("0 pool WSL VMs ended, 0 not, 0 unmapped"),
        "{line}"
    );
}

/// A VM no profile holds a folder for.
const HIDDEN_VM: &str = "11111111-2222-4333-8444-555555555555";
/// Another Windows account on the machine, not the lawyer, not a pool user.
const OTHER: u32 = 1050;

/// [`with_vms`], with [`OTHER`]'s profile listed too.
fn with_other(systems: &[&str]) -> Fake {
    let mut fake = with_vms(systems);
    let listed = [Sid::parse(OWNER).unwrap(), sid(1010), sid(1011), sid(OTHER)];
    fake.vms.profiles = Some(Ok(listed.to_vec()));
    fake
}

#[test]
fn a_vm_in_no_profile_is_ended_and_every_pool_user_marked() {
    // No folder names it: most likely a pool user's VM that hid its folder.
    let mut fake = with_other(&[HIDDEN_VM, POOL_VM]);
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(ended(&fake), [POOL_VM, HIDDEN_VM]);
    assert_eq!(fake.resets, [sid(1011), sid(1010)]);
    // Both marked before it was terminated.
    let at = |line: &str| fake.done.iter().position(|done| done == line);
    let hidden = at(&format!("end vm {HIDDEN_VM}"));
    let marks = fake.done.iter().filter(|done| done.starts_with("mark"));
    let last_mark = fake.done.iter().rposition(|done| done.starts_with("mark"));
    assert_eq!(marks.count(), 2, "{:?}", fake.done);
    assert!(last_mark < hidden, "{:?}", fake.done);
    let line = &fake.logged[0].1;
    assert!(
        line.contains("2 pool WSL VMs ended, 0 not, 0 unmapped, 1 in no profile"),
        "{line}"
    );
    // Its termination failing is 48, the pool still marked.
    let mut fake = with_other(&[HIDDEN_VM]);
    fake.vms.refuse = vec![HIDDEN_VM.to_owned()];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    let line = &fake.logged[0].1;
    assert!(line.contains("0 pool WSL VMs ended, 1 not"), "{line}");
    // A WSL system whose id isn't a GUID can't be named to HCS: unmapped, 48.
    let mut fake = with_vms(&["wsl-container"]);
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(ended(&fake), Vec::<&str>::new());
    assert!(fake.logged[0].1.contains("1 unmapped"), "{:?}", fake.logged);
}

#[test]
fn a_vm_another_non_pool_account_holds_is_left_and_a_planted_copy_marks_that_user() {
    let mut fake = with_other(&[HIDDEN_VM]);
    fake.vms.other_folders = vec![(sid(OTHER), HIDDEN_VM.to_owned())];
    fake.vms.folders.push((sid(1010), HIDDEN_VM.to_owned()));
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(ended(&fake), Vec::<&str>::new());
    assert_eq!(fake.resets, [sid(1010)]);
    let line = &fake.logged[0].1;
    assert!(
        line.contains("0 not, 0 unmapped, 0 in no profile"),
        "{line}"
    );
}

#[test]
fn a_non_pool_profile_that_cant_be_read_leaves_every_vm_in_no_profile_running_at_48() {
    // Another account's profile can't be read: the VM in no profile may be that account's.
    let mut fake = with_other(&[HIDDEN_VM, POOL_VM]);
    fake.vms.other_unreadable = vec![sid(OTHER)];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    // The pool user's VM is still ended: another account's profile that can't be read is
    // passed over in matching.
    assert_eq!(ended(&fake), [POOL_VM]);
    assert_eq!(fake.resets, [sid(1011), sid(1010)]);
    let line = &fake.logged[0].1;
    assert!(
        line.contains("1 pool WSL VMs ended, 0 not, 1 unmapped, 0 in no"),
        "{line}"
    );
    assert!(line.contains("WSL VM's user isn't known"), "{line}");
    // The list of profiles failing is the same: nothing in no profile is ended.
    let mut fake = with_vms(&[HIDDEN_VM, POOL_VM]);
    fake.vms.profiles = Some(Err("RegEnumKeyExW (ProfileList) failed (5)".to_owned()));
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(ended(&fake), [POOL_VM]);
    assert_eq!(fake.resets, [sid(1011), sid(1010)]);
    assert!(fake.logged[0].1.contains("1 unmapped"), "{:?}", fake.logged);
    // One read failing for one VM's id holds back the VM in no profile seen before it too.
    let mut fake = with_vms(&[HIDDEN_VM, LAWYER_VM]);
    fake.vms.owner_folders.clear();
    fake.vms.owner_unreadable = true;
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(ended(&fake), Vec::<&str>::new());
}

#[test]
fn a_vm_whose_lawyers_folder_cant_be_read_is_unmapped_even_with_a_pool_users_folder() {
    let mut fake = with_vms(&[POOL_VM]);
    fake.vms.owner_unreadable = true;
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(ended(&fake), Vec::<&str>::new());
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    assert!(fake.logged[0].1.contains("1 unmapped"), "{:?}", fake.logged);
}

#[test]
fn a_vm_a_pool_users_unreadable_profile_may_hold_is_ended_and_every_user_marked() {
    // In no profile it is ended, every pool user marked, the unreadable one included.
    let mut fake = with_vms(&[HIDDEN_VM]);
    fake.vms.unreadable = vec![sid(1010)];
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(ended(&fake), [HIDDEN_VM]);
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    // With no VM at all, nothing is read, so nothing is unknown.
    let mut fake = with_vms(&[]);
    fake.vms.unreadable = vec![sid(1010)];
    assert_eq!(stop(&mut fake), StopExit::Done);
}

#[test]
fn a_list_that_fails_is_48_with_wsl_installed_and_passed_over_without() {
    for wsl in [true, false] {
        let mut fake = with_vms(&[]);
        fake.vms.systems = Err("HcsWaitForOperationResult (the list) failed".to_owned());
        fake.vms.wsl = wsl;
        let exit = if wsl {
            StopExit::NotStopped
        } else {
            StopExit::Done
        };
        assert_eq!(stop(&mut fake), exit, "WSL installed: {wsl}");
        let line = &fake.logged[0].1;
        assert_eq!(line.contains("couldn't be listed"), wsl, "{line}");
    }
}

#[test]
fn only_a_guid_is_a_vm_id_and_it_is_held_upper_case_without_braces() {
    let id = VmId::parse("{6b29fc40-ca47-1067-b31d-00dd010662da}").unwrap();
    assert_eq!(id.as_str(), POOL_VM);
    assert_eq!(VmId::parse(POOL_VM).unwrap().as_str(), POOL_VM);
    let refused = [
        "",
        "{}",
        "..\\..\\x",
        "6B29FC40-CA47-1067-B31D-00DD010662D",
        "6B29FC40-CA47-1067-B31D-00DD010662DA0",
        "6B29FC40CA47-1067-B31D-00DD010662DA-",
        "6B29FC40-CA47-1067-B31D-00DD010662DG",
        "{6B29FC40-CA47-1067-B31D-00DD010662DA",
        "6B29FC40-CA47-1067-B31D-00DD010662DA}",
        "6B29FC40-CA47-1067-B31D-00DD01066\\DA",
    ];
    for text in refused {
        assert_eq!(VmId::parse(text), None, "{text}");
    }
}

#[test]
fn the_lists_wsl_ids_are_read_from_hcs_json_and_anything_else_is_refused() {
    let document = r#"[{"Id":"A","SystemType":"VirtualMachine","Owner":"WSL"},{"Id":"b"},
        {"Id":"C","Owner":"docker"},{"Id":"D","Owner":"wsl"},{"Id":"E","Owner":"WSL"}]"#;
    assert_eq!(wsl_ids_from_json(document).unwrap(), ["A", "E"]);
    assert_eq!(wsl_ids_from_json("[]").unwrap(), Vec::<String>::new());
    for bad in ["", "{}", r#"[{"Owner":"WSL"}]"#, "[1]"] {
        let why = wsl_ids_from_json(bad).unwrap_err();
        assert!(!why.contains("WSL"), "{why}");
    }
}

#[test]
fn the_lawyers_local_app_data_redirect_is_expanded_against_its_profile_only() {
    let profile = r"C:\Users\lawyer";
    let expanded = |value: &str| local_app_data(value, profile);
    assert_eq!(
        expanded(r"%USERPROFILE%\AppData\Local").as_deref(),
        Some(r"C:\Users\lawyer\AppData\Local")
    );
    assert_eq!(
        expanded(r" %userprofile%\Local ").as_deref(),
        Some(r"C:\Users\lawyer\Local")
    );
    assert_eq!(
        expanded(r"D:\Data\Local").as_deref(),
        Some(r"D:\Data\Local")
    );
    assert_eq!(
        expanded(r"\\server\share\lawyer").as_deref(),
        Some(r"\\server\share\lawyer")
    );
    for refused in [
        r"%LOCALAPPDATA%",
        r"%HOMEDRIVE%%HOMEPATH%\Local",
        r"%USERPROFILE%\%X%",
        r"AppData\Local",
        r"C:",
        "",
    ] {
        assert_eq!(expanded(refused), None, "{refused}");
    }
}

/// A pool user's WSL process as the snapshot sees it.
fn wsl_process(pid: u32, user: u32) -> Process {
    Process {
        image: Image::Path(r"C:\Program Files\WSL\wslhost.exe".to_owned()),
        wsl: true,
        ..seen(pid, Some(sid(user)))
    }
}

#[test]
fn with_no_pool_wsl_evidence_hcs_is_never_asked_and_nothing_is_ended() {
    let mut fake = with_vms(&[POOL_VM, HIDDEN_VM]);
    fake.vms.temp_guids.clear();
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.vms.hcs_calls, 0);
    assert_eq!(fake.vms.reads + fake.vms.owner_reads, 0);
    assert_eq!(ended(&fake), Vec::<&str>::new());
    let line = &fake.logged[0].1;
    assert!(
        line.contains("WSL VM step skipped: no pool WSL evidence"),
        "{line}"
    );
    // The lawyer's own WSL process is no evidence; neither is a pool user's other process.
    let mut fake = with_vms(&[POOL_VM]);
    fake.vms.temp_guids.clear();
    let lawyer = Process {
        wsl: true,
        ..seen(12, Some(Sid::parse(OWNER).unwrap()))
    };
    fake.snapshots = vec![Ok(vec![lawyer, seen(13, Some(sid(1010)))]), Ok(Vec::new())];
    stop(&mut fake);
    assert_eq!(fake.vms.hcs_calls, 0);
}

#[test]
fn each_kind_of_evidence_starts_the_step_and_is_recorded_before_the_kill_pass() {
    // (a) a GUID folder in a pool user's temp, (c) a distro in its hive: read before the
    // first snapshot.
    for kind in ["temp", "lxss"] {
        let mut fake = with_vms(&[POOL_VM]);
        fake.vms.temp_guids.clear();
        if kind == "lxss" {
            fake.vms.lxss = vec![sid(1010)];
        } else {
            fake.vms.temp_guids = vec![sid(1010)];
        }
        assert_eq!(stop(&mut fake), StopExit::Done, "{kind}");
        assert_eq!(ended(&fake), [POOL_VM], "{kind}");
        let first = fake.done.iter().position(|done| done == "snapshot");
        let read = fake
            .done
            .iter()
            .position(|done| done.starts_with("evidence"));
        assert!(read < first, "{kind}: {:?}", fake.done);
    }
    // A read that fails counts as evidence.
    let mut fake = with_vms(&[POOL_VM]);
    fake.vms.temp_guids.clear();
    fake.vms.unreadable = vec![sid(1010)];
    stop(&mut fake);
    assert!(fake.vms.hcs_calls > 0);
    // (b) a pool user's WSL process, seen by the first snapshot and gone after the kill pass:
    // the step still runs.
    let mut fake = with_vms(&[POOL_VM]);
    fake.vms.temp_guids.clear();
    fake.snapshots = vec![Ok(vec![wsl_process(21, 1011)]), Ok(Vec::new())];
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.stopped(), [21]);
    assert_eq!(ended(&fake), [POOL_VM]);
    // A pico (subsystem) process is WSL's too, as are WSL's exes in any case.
    assert!(wsl_image(&Image::Subsystem));
    assert!(wsl_image(&Image::Path(r"C:\x\WSLRelay.EXE".to_owned())));
    assert!(wsl_image(&Image::Path("wslg.exe".to_owned())));
    for other in [
        Image::Missing,
        Image::Unread,
        Image::Path("wsl.exe.bak".to_owned()),
    ] {
        assert!(!wsl_image(&other), "{other:?}");
    }
}
