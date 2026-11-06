//! The set-up engine against [`fake::FakeMachine`] and [`fake::FakeAcl`], on every OS: the
//! fresh run (`create.rs`), the rollback after a failure at each step (`rollback.rs`), and the
//! extend and re-run (`extend.rs`).

use std::path::PathBuf;

use super::*;
use crate::windows::acl_plan::Rights;
use crate::windows::fence::MatterFolders;

mod create;
mod extend;
mod fake;
mod remains;
mod rollback;

use fake::{FakeAcl, FakeMachine, OWNER, OWNER_NAME, sid};

/// The owner's SID.
fn owner() -> Sid {
    Sid::parse(OWNER).unwrap()
}

/// A set-up (or with `add`, an extend) for the owner over `machine`'s data folder.
fn request(machine: &FakeMachine, add: u8, matters: Vec<RegisteredMatter>) -> SetupRequest {
    SetupRequest {
        owner: owner(),
        data_dir: machine.data_dir(),
        add,
        matters,
    }
}

/// A fresh set-up that succeeds, for the tests that start from a pool.
fn set_up_fresh(machine: &mut FakeMachine, acl: &mut FakeAcl) -> SetupReport {
    let req = request(machine, 0, Vec::new());
    set_up(machine, acl, &req).unwrap()
}

/// The `LocalLow` of `machine`'s profile for `name`.
fn local_low(machine: &FakeMachine, name: &str) -> PathBuf {
    machine.profile_path(name).join("AppData").join("LocalLow")
}

/// The calls a fresh set-up of four users makes, in order.
fn fresh_calls() -> Vec<String> {
    let mut calls: Vec<String> = ["lock", "read_record", "seclogon", "logon_right", "group"]
        .into_iter()
        .chain(["create_group"])
        .map(str::to_owned)
        .collect();
    let mut recorded = Vec::new();
    for n in 1..=4u32 {
        let name = format!("ClauseoSb0{n}");
        recorded.push((1000 + n).to_string());
        calls.extend([
            format!("account_sid {name}"),
            format!("create_user {name}"),
            format!("add_to_group {name}"),
            format!("hide {name}"),
            format!("write_record {}", recorded.join(",")),
            format!("log_on_once {name}"),
        ]);
    }
    calls.extend(
        [
            "program_data",
            "journal_volumes",
            "journal_size C:",
            "grow_journal C:",
            "protect_for_owner 1001,1002,1003,1004",
            "write_marker",
            "unlock",
        ]
        .map(str::to_owned),
    );
    calls
}

/// Two registered matters with their own folders in `machine`'s temp folder, held by pool users
/// 1001 and 1002.
fn two_matters(machine: &FakeMachine) -> Vec<RegisteredMatter> {
    let ids = [
        "01890a5d-ac96-774b-bcce-b302099a8057",
        "01890a5d-ac96-774b-8cce-b302099a8058",
    ];
    ids.iter()
        .zip([1001, 1002])
        .enumerate()
        .map(|(n, (id, user))| {
            let root = machine.dir.dir(&format!("matters/m{n}"));
            let state = machine.dir.dir(&format!("data/agent-state/{id}/claude"));
            RegisteredMatter {
                id: id.parse().unwrap(),
                folders: MatterFolders {
                    root,
                    state,
                    notes: None,
                },
                pool_user: sid(user),
                fenced: true,
            }
        })
        .collect()
}
