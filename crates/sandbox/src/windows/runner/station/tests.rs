//! The window station's steps, in order, with the thread itself again on every path.

use super::*;

const POOL: &str = "S-1-5-21-1111111111-2222222222-3333333333-1005";
const LAWYER: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
const MATTER: &str = "S-1-5-21-444444444-555555555-666666666-1234";
const NAME: &str = "Service-0x0-1a2b3c$";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// Records each step, with the descriptor or name it was given; fails the one named `fail`.
#[derive(Default)]
struct Record {
    done: Vec<String>,
    fail: Option<&'static str>,
    /// Whether the thread acts as the pool user now.
    impersonating: bool,
}

impl Record {
    fn step(&mut self, name: &str, given: &str) -> Result<(), String> {
        if given.is_empty() {
            self.done.push(name.to_owned());
        } else {
            self.done.push(format!("{name} {given}"));
        }
        if self.fail == Some(name) {
            return Err(name.to_owned());
        }
        Ok(())
    }
}

impl StationSteps for Record {
    type Logon = u32;
    type Station = &'static str;
    type Desktop = u32;
    type Error = String;

    fn log_on(&mut self) -> Result<u32, String> {
        self.step("log on", "").map(|()| 3)
    }
    fn impersonate(&mut self, logon: &u32) -> Result<(), String> {
        assert_eq!(*logon, 3);
        self.step("impersonate", "")?;
        self.impersonating = true;
        Ok(())
    }
    fn create_station(&mut self, sddl: &str) -> Result<&'static str, String> {
        assert!(self.impersonating, "the station is made as the pool user");
        self.step("create station", sddl).map(|()| "created")
    }
    fn revert(&mut self) {
        self.impersonating = false;
        self.done.push("revert".to_owned());
    }
    fn close_logon(&mut self, logon: u32) {
        assert_eq!(logon, 3);
        self.done.push("close logon".to_owned());
    }
    fn name(&mut self, station: &&'static str) -> Result<String, String> {
        assert_eq!(*station, "created");
        self.step("name", "").map(|()| NAME.to_owned())
    }
    fn open_station(&mut self, name: &str) -> Result<&'static str, String> {
        assert!(!self.impersonating, "the station is opened as the lawyer");
        self.step("open station", name).map(|()| "opened")
    }
    fn set_owner(&mut self, station: &&'static str, owner: &Sid) -> Result<(), String> {
        assert_eq!(*station, "opened");
        self.step("set owner", owner.as_str())
    }
    fn create_desktop(&mut self, station: &&'static str, sddl: &str) -> Result<u32, String> {
        assert_eq!(*station, "opened");
        self.step("create desktop", sddl).map(|()| 9)
    }
    fn secure_station(&mut self, station: &&'static str, sddl: &str) -> Result<(), String> {
        assert_eq!(*station, "opened");
        self.step("secure station", sddl)
    }
    fn secure_desktop(&mut self, desktop: &u32, sddl: &str) -> Result<(), String> {
        assert_eq!(*desktop, 9);
        self.step("secure desktop", sddl)
    }
}

fn run(record: &mut Record) -> Result<Made<&'static str, u32>, String> {
    let (creator, pool_user, matter) = (sid(LAWYER), sid(POOL), sid(MATTER));
    make_station(
        record,
        &StationSids {
            creator: &creator,
            pool_user: &pool_user,
            matter: Some(&matter),
        },
    )
}

/// Each step's name, without what it was given.
fn names(record: &Record) -> Vec<String> {
    record
        .done
        .iter()
        .map(|step| {
            let known = ORDER.iter().find(|name| step.starts_with(*name));
            (*known.unwrap_or(&step.as_str())).to_owned()
        })
        .collect()
}

const ORDER: [&str; 11] = [
    "log on",
    "impersonate",
    "create station",
    "revert",
    "close logon",
    "name",
    "open station",
    "set owner",
    "create desktop",
    "secure station",
    "secure desktop",
];

#[test]
fn the_steps_run_in_order_and_give_the_station_by_its_name() {
    let mut record = Record::default();
    let made = run(&mut record).unwrap();
    assert_eq!(
        (made.station, made.desktop, made.name.as_str()),
        ("opened", 9, NAME)
    );
    assert_eq!(names(&record), ORDER);
    assert!(!record.impersonating);
}

#[test]
fn each_step_gets_its_descriptor() {
    let mut record = Record::default();
    run(&mut record).unwrap();
    let (lawyer, pool, matter) = (sid(LAWYER), sid(POOL), sid(MATTER));
    let given = |name: &str| {
        record
            .done
            .iter()
            .find_map(|step| step.strip_prefix(&format!("{name} ")))
            .unwrap()
            .to_owned()
    };
    assert_eq!(given("create station"), station_creation_sddl(&lawyer));
    assert_eq!(given("open station"), NAME);
    assert_eq!(given("set owner"), LAWYER);
    assert_eq!(given("create desktop"), desktop_creation_sddl(&lawyer));
    assert_eq!(
        given("secure station"),
        station_sddl(&lawyer, &pool, Some(&matter))
    );
    assert_eq!(
        given("secure desktop"),
        desktop_sddl(&lawyer, &pool, Some(&matter))
    );
}

#[test]
fn every_failure_refuses_and_runs_nothing_after_it() {
    for (at, step) in ORDER.iter().enumerate() {
        if ["revert", "close logon"].contains(step) {
            continue;
        }
        let mut record = Record {
            fail: Some(step),
            ..Record::default()
        };
        assert_eq!(run(&mut record).err(), Some((*step).to_owned()), "{step}");
        let mut want: Vec<String> = ORDER.iter().take(at + 1).map(|s| (*s).to_owned()).collect();
        // A failure while acting as the pool user still reverts, then closes the logon.
        if ["impersonate", "create station"].contains(step) {
            want.extend(["revert".to_owned(), "close logon".to_owned()]);
        }
        assert_eq!(names(&record), want, "{step}");
    }
}

#[test]
fn the_thread_is_itself_again_after_every_failure() {
    for step in ORDER {
        let mut record = Record {
            fail: Some(step),
            ..Record::default()
        };
        drop(run(&mut record));
        assert!(!record.impersonating, "{step}");
        let reverts = record.done.iter().filter(|s| *s == "revert").count();
        let impersonated = record.done.iter().any(|s| s == "impersonate");
        assert_eq!(reverts, usize::from(impersonated), "{step}");
    }
}

#[test]
fn a_failed_logon_impersonates_nothing() {
    let mut record = Record {
        fail: Some("log on"),
        ..Record::default()
    };
    assert_eq!(run(&mut record).err(), Some("log on".to_owned()));
    assert_eq!(record.done, ["log on"]);
}
