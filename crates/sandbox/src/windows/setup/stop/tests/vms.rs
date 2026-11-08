//! The stop's WSL step (step 5b, `vms.rs`) through the fake: a VM whose folder a pool user's
//! profile holds is ended and its user marked, every other VM (the lawyer's) is left alone, a
//! VM that isn't ended or can't be mapped is 48, a list that fails is 48 only with WSL
//! installed, and with no HCS nothing is asked.

use super::*;

/// The lawyer's VM: no pool user's profile holds its folder.
const LAWYER_VM: &str = "0F0E0D0C-0B0A-4908-8706-050403020100";
/// A pool user's VM, as WSL names its folder (upper case, no braces).
const POOL_VM: &str = "6B29FC40-CA47-1067-B31D-00DD010662DA";

/// HCS on the fake machine, and the pool's VM folders.
pub(super) struct FakeVms {
    /// `Stopper::compute_systems`: `Ok(None)` is no HCS.
    pub(super) systems: Result<Option<Vec<String>>, Failed>,
    /// `WSLService` or `LxssManager` is installed.
    pub(super) wsl: bool,
    /// The folders `AppData\Local\temp\<id>` each pool user's profile holds.
    pub(super) folders: Vec<(Sid, String)>,
    /// Pool users whose profile can't be read.
    pub(super) unreadable: Vec<Sid>,
    /// Ids whose termination fails.
    pub(super) refuse: Vec<String>,
    /// Ids that never end: each termination waits out its wait, on the fake clock.
    pub(super) hang: Vec<String>,
    /// The folder reads, as (user's rid, id).
    pub(super) reads: usize,
}

impl Default for FakeVms {
    fn default() -> FakeVms {
        FakeVms {
            systems: Ok(None),
            wsl: true,
            folders: Vec::new(),
            unreadable: Vec::new(),
            refuse: Vec::new(),
            hang: Vec::new(),
            reads: 0,
        }
    }
}

impl VmStopper for Fake {
    fn compute_systems(&mut self) -> Result<Option<Vec<String>>, Failed> {
        self.vms.systems.clone()
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

    fn terminate_vm(&mut self, id: &VmId, wait: Duration) -> Result<(), Failed> {
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
/// 1011's profile holds `POOL_VM`'s folder.
fn with_vms(systems: &[&str]) -> Fake {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![Ok(vec![seen(11, Some(Sid::parse(OWNER).unwrap()))])];
    let systems = systems.iter().map(|id| (*id).to_owned()).collect();
    fake.vms.systems = Ok(Some(systems));
    fake.vms.folders = vec![(sid(1011), POOL_VM.to_owned())];
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
    // HCS's id for the pool's VM in another case and with braces; an id that isn't a GUID (a
    // container's) isn't WSL's.
    let braced = format!("{{{}}}", POOL_VM.to_ascii_lowercase());
    let mut fake = with_vms(&[LAWYER_VM, &braced, "docker-0123"]);
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(ended(&fake), [POOL_VM]);
    assert_eq!(fake.waits, [VM_WAIT]);
    assert_eq!(fake.resets, [sid(1011)]);
    // Each pool user is read for the lawyer's VM; 1010 then 1011 for the pool's.
    assert_eq!(fake.vms.reads, 4);
    let (error, line) = &fake.logged[0];
    assert!(!error, "{line}");
    assert!(
        line.contains("0 not, 1 pool WSL VMs ended, 0 not, 0 unmapped; 1 users marked"),
        "{line}"
    );
    assert!(
        !line.contains(POOL_VM) && !line.contains("S-1-5"),
        "counts only: {line}"
    );
    assert!(fake.completed().is_some());
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
fn a_vm_a_pool_users_unreadable_profile_may_hold_is_48_and_that_user_marked() {
    let mut fake = with_vms(&[LAWYER_VM]);
    fake.vms.unreadable = vec![sid(1010)];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(ended(&fake), Vec::<&str>::new());
    assert_eq!(fake.resets, [sid(1010)]);
    assert!(fake.logged[0].1.contains("1 unmapped"), "{:?}", fake.logged);
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
fn the_lists_ids_are_read_from_hcs_json_and_anything_else_is_refused() {
    let document = r#"[{"Id":"A","SystemType":"VirtualMachine","Owner":"WSL"},{"Id":"b"}]"#;
    assert_eq!(ids_from_json(document).unwrap(), ["A", "b"]);
    assert_eq!(ids_from_json("[]").unwrap(), Vec::<String>::new());
    for bad in ["", "{}", r#"[{"Owner":"WSL"}]"#, "[1]"] {
        let why = ids_from_json(bad).unwrap_err();
        assert!(!why.contains("WSL"), "{why}");
    }
}
