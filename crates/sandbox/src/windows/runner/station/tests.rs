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
    /// Whether this process is on the agent's station now.
    entered: bool,
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
    type Previous = &'static str;
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
    fn enter_station(&mut self, station: &&'static str) -> Result<&'static str, String> {
        assert_eq!(*station, "opened");
        self.step("enter station", "")?;
        self.entered = true;
        Ok("own")
    }
    fn create_desktop(&mut self, sddl: &str) -> Result<u32, String> {
        assert!(self.entered, "the desktop is made on the agent's station");
        self.step("create desktop", sddl).map(|()| 9)
    }
    fn leave_station(&mut self, previous: &'static str) {
        assert_eq!(previous, "own");
        self.entered = false;
        self.done.push("leave station".to_owned());
    }
    fn secure_station(&mut self, station: &&'static str, sddl: &str) -> Result<(), String> {
        assert_eq!(*station, "opened");
        assert!(!self.entered, "the outer is back on its own station");
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

const ORDER: [&str; 13] = [
    "log on",
    "impersonate",
    "create station",
    "revert",
    "close logon",
    "name",
    "open station",
    "set owner",
    "enter station",
    "create desktop",
    "leave station",
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
        if ["revert", "close logon", "leave station"].contains(step) {
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
        // A failure on the agent's station still switches back.
        if *step == "create desktop" {
            want.push("leave station".to_owned());
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
        assert!(!record.entered, "{step}");
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

#[test]
fn the_outer_leaves_the_agents_station_on_every_path_once_it_entered() {
    for step in ORDER {
        let mut record = Record {
            fail: Some(step),
            ..Record::default()
        };
        drop(run(&mut record));
        let leaves = record.done.iter().filter(|s| *s == "leave station").count();
        let entered = record.done.iter().any(|s| s == "enter station") && step != "enter station";
        assert_eq!(leaves, usize::from(entered), "{step}");
    }
}

#[test]
fn a_default_desktop_someone_made_first_refuses() {
    assert_eq!(made_fresh(0), Ok(()));
    assert_eq!(made_fresh(183), Err(183));
    // Any other code left over from an earlier call isn't this one's.
    assert_eq!(made_fresh(5), Ok(()));
}

#[test]
fn the_inner_takes_only_a_station_default_as_the_launch_desktop() {
    let path = r"Service-0x0-1a2b3c$\Default";
    assert_eq!(StationDesktop::from_path(path).unwrap().as_str(), path);
    assert!(StationDesktop::from_path(r"Service-0x1-ffff$\Default").is_some());
    for not in [
        "",
        r"\Default",
        "Default",
        r"WinSta0\Default",
        r"WinSta0\Clauseo",
        r"Service-0x0-1a2b3c\Default",
        r"ClauseoRun-0123456789abcdef\Default",
        r"Service-0x0-1a2b3c$\Clauseo",
        r"Service-0x0-1a2b3c$",
        r"Service-0x0-1a2b3c$\Default\Default",
    ] {
        assert_eq!(StationDesktop::from_path(not), None, "{not}");
    }
}

#[test]
fn a_runner_desktop_is_only_ever_default_on_an_unnamed_station() {
    let desktop = StationDesktop::of(NAME).unwrap();
    assert_eq!(desktop.as_str(), r"Service-0x0-1a2b3c$\Default");
    let wide = desktop.wide();
    let (nul, text) = wide.split_last().unwrap();
    assert_eq!(*nul, 0);
    assert_eq!(String::from_utf16(text).unwrap(), desktop.as_str());
    assert!(StationDesktop::of("Service-0x1-ffff$").is_some());
    for not in [
        "",
        "WinSta0",
        r"WinSta0\Default",
        "Service-0x0-$",
        "Service-0x0-1a2b3c",
        r"Service-0x0-1a\b$",
        "ClauseoRun-0123456789abcdef",
        "service-0x0-1a$",
    ] {
        assert_eq!(StationDesktop::of(not), None, "{not}");
    }
}

#[test]
fn a_station_name_is_the_last_component_of_its_object_path() {
    let path = r"\Sessions\1\Windows\WindowStations\Service-0x0-311a3$";
    assert_eq!(station_name_of_path(path), Ok("Service-0x0-311a3$"));
    assert_eq!(
        station_name_of_path(r"\Windows\WindowStations\Service-0x1-ffff$"),
        Ok("Service-0x1-ffff$")
    );
    let name = station_name_of_path(path).unwrap();
    assert!(name.ends_with('$'));
    assert!(StationDesktop::of(name).is_some());
}

#[test]
fn a_station_name_path_without_the_window_stations_part_is_refused() {
    for not in [
        "",
        "Service-0x0-311a3$",
        r"\Sessions\1\Windows\Service-0x0-311a3$",
        r"\Sessions\1\Windows\WindowStations",
        r"\Sessions\1\Windows\WindowStations\\Service-0x0-311a3$",
        r"\Sessions\1\WindowStations-old\Service-0x0-311a3$",
    ] {
        assert!(station_name_of_path(not).is_err(), "{not}");
    }
}

#[test]
fn a_station_name_path_with_no_name_or_a_deeper_one_is_refused() {
    for not in [
        r"\Sessions\1\Windows\WindowStations\",
        r"\Sessions\1\Windows\WindowStations\Service-0x0-311a3$\Default",
        r"\Sessions\1\Windows\WindowStations\Service-0x0-311a3$\",
    ] {
        assert!(station_name_of_path(not).is_err(), "{not}");
    }
}

#[test]
fn an_object_name_must_lie_wholly_inside_the_buffer() {
    // An 8 KiB buffer at 0x1000; the header is 16 bytes, the name right after it.
    let (base, size) = (0x1000, 8192);
    assert_eq!(name_span(base, size, base + 16, 40), Ok((8, 20)));
    // The very end of the buffer is fine; one unit past it isn't.
    assert_eq!(
        name_span(base, size, base + size - 40, 40),
        Ok(((size - 40) / 2, 20))
    );
    for (at, bytes) in [
        (base + size - 38, 40), // runs past the end
        (base - 2, 40),         // starts before the buffer
        (base + size, 2),       // starts at the end
        (base + 16, 0),         // empty
        (base + 16, 41),        // odd length
        (base + 17, 40),        // odd offset
        (usize::MAX - 1, 40),   // wraps
    ] {
        assert!(name_span(base, size, at, bytes).is_err(), "{at:#x} {bytes}");
    }
}
