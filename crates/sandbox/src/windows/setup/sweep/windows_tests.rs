//! The sweep's task step on a real machine (`windows_sweeper.rs`): a test user's scheduled task,
//! made as that user, is deleted when the pool is that user's SID, and the task this process's
//! user (the lawyer, here an elevated administrator) made beside it is left. And this machine's
//! own tasks all have a principal that reads and resolves, and every task on file and in the
//! Task Scheduler's cache is listed (the cross-check), so a real run doesn't end 56 on a clean
//! machine. And a test user's task whose descriptor shuts SYSTEM and Administrators out is
//! deleted or 56, never left behind a sweep that would end 0.
//!
//! The gate is skipped (it needs LOCAL SYSTEM and a `ClauseoSbNN` pool), as the stop's BITS test
//! skips it: the step is called with the pool the test chose. Elevated, as CI's elevated step
//! runs it (`CLAUSEO_ELEVATED_TESTS=1`): making an account needs it.

use std::process::Command;

mod forged;

use super::windows_listing::is_guid;
use super::windows_sweeper::WindowsSweeper;
use super::windows_sweeper::same_task_path;
use super::{Names, Resolved, Sweeper as _, Task, resolve, sddl_owner, sweep_tasks};
use crate::test_support::TempDir;
use crate::windows::refresh::windows_tests::{system32, try_in_cmd};
use crate::windows::setup::logon::tests::{TestUser, clear_leftovers, random_hex};
use crate::windows::setup::strays::TASK_TEST_PREFIX;
use crate::windows::test_dacl::{elevated, protect_with};

/// Whether the test may run here: an elevated token and `CLAUSEO_ELEVATED_TESTS=1`.
fn gate(test: &str) -> bool {
    let asked = std::env::var("CLAUSEO_ELEVATED_TESTS").is_ok_and(|value| value == "1");
    if asked && elevated() {
        return true;
    }
    eprintln!("skipped {test}: needs an elevated token and CLAUSEO_ELEVATED_TESTS=1");
    false
}

/// The tasks the test made, deleted on drop as this administrator, whoever made them; one
/// already gone is no error.
struct Made(Vec<String>);

impl Drop for Made {
    #[expect(
        clippy::disallowed_methods,
        reason = "the clean-up of the test's own tasks, through schtasks"
    )]
    fn drop(&mut self) {
        for name in &self.0 {
            let deleted = Command::new(format!(r"{}\schtasks.exe", system32()))
                .args(["/delete", "/tn", name, "/f"])
                .output();
            if let Ok(output) = deleted
                && output.status.success()
            {
                eprintln!("the clean-up deleted the task {name}");
            }
        }
    }
}

/// The root folder's tasks, as the sweep reads them.
fn root_tasks(sweeper: &mut WindowsSweeper) -> Vec<Task> {
    let root = sweeper.root().unwrap();
    sweeper.tasks(&root).unwrap()
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
#[expect(
    clippy::disallowed_methods,
    reason = "makes the lawyer's own task through schtasks, as this process"
)]
fn elevated_the_sweep_deletes_a_pool_users_task_and_leaves_the_lawyers() {
    if !gate("elevated_the_sweep_deletes_a_pool_users_task_and_leaves_the_lawyers") {
        return;
    }
    // 1. A standard account, as the stop's BITS test makes one.
    clear_leftovers(TASK_TEST_PREFIX);
    let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let sid = user.sid.clone();
    eprintln!("1. the test user: {} {}", user.name, sid.as_str());
    let dir = TempDir::new("sweep-tasks");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();
    let suffix = format!("{}{}", std::process::id(), random_hex(2));
    let theirs = format!("ClauseoSweepPool{suffix}");
    let mine = format!("ClauseoSweepLawyer{suffix}");
    // Dropped before the user, so the account outlives its task's clean-up.
    let _made = Made(vec![theirs.clone(), mine.clone()]);
    let schtasks = format!(r"{}\schtasks.exe", system32());
    let action = format!(r"{}\cmd.exe /c exit 0", system32());

    // 2. As that user, unelevated: a task of its own, in the root folder.
    let create =
        format!(r#""{schtasks}" /create /tn {theirs} /tr "{action}" /sc once /st 23:59 /f"#);
    let (code, printed) = try_in_cmd(&user, dir.path(), &create);
    eprintln!("2. as the test user: {create}\nexit {code:?}\n{printed}");
    assert_eq!(
        code,
        Some(0),
        "schtasks /create as the test user:\n{printed}"
    );

    // 3. As this process, the lawyer: a task beside it.
    let output = Command::new(&schtasks)
        .args(["/create", "/tn", &mine, "/tr", &action])
        .args(["/sc", "once", "/st", "23:59", "/f"])
        .output()
        .unwrap();
    eprintln!("3. as this process: {:?}", output.status);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 4. Both are read, and each principal resolves: the test user's to its SID, the lawyer's
    // to another.
    let mut sweeper = WindowsSweeper::new();
    let listed = root_tasks(&mut sweeper);
    let names = Names::of(&mut sweeper, std::slice::from_ref(&sid));
    assert_eq!(
        names.users.len(),
        1,
        "the test user's name is read: {names:?}"
    );
    for (name, pool) in [(&theirs, true), (&mine, false)] {
        let task = listed.iter().find(|task| &task.name == name).unwrap();
        let principal = task.principal.clone().unwrap();
        eprintln!("4. {name}: {principal:?}");
        let named = principal.name().unwrap();
        let Resolved::Sid(resolved) = resolve(&mut sweeper, &names, named) else {
            panic!("{name}: its principal resolves to no SID");
        };
        assert_eq!(resolved == sid, pool, "{name}: {}", resolved.as_str());
    }

    // 5. The sweep's task step, with the test user as the pool.
    let swept = sweep_tasks(&mut sweeper, std::slice::from_ref(&sid));
    eprintln!("5. {swept:?}");
    assert!(swept.deleted >= 1, "{swept:?}");
    assert_eq!(swept.not_deleted, 0, "{swept:?}");
    let left = root_tasks(&mut sweeper);
    assert!(
        !left.iter().any(|task| task.name == theirs),
        "the pool's task is still there"
    );
    assert!(
        left.iter().any(|task| task.name == mine),
        "the lawyer's task is gone"
    );
    eprintln!("6. the clean-up deletes the lawyer's task, then the user");
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_this_machines_tasks_all_have_a_principal_that_resolves() {
    if !gate("elevated_this_machines_tasks_all_have_a_principal_that_resolves") {
        return;
    }
    // With an empty pool nothing is deleted, but every principal is still read and resolved:
    // one that isn't would end every real run at 56. Each failure is listed by its kind (the
    // call that failed), never by a task's or an account's name.
    let mut sweeper = WindowsSweeper::new();
    let swept = sweep_tasks(&mut sweeper, &[]);
    eprintln!(
        "{} folders, {} tasks, {} with no user or group, {} unreadable, {} skipped, {} folders \
         unlisted",
        swept.folders,
        swept.tasks,
        swept.no_principal,
        swept.unreadable,
        swept.skipped,
        swept.unlisted
    );
    for (why, count) in &swept.why {
        eprintln!("{count} x {why}");
    }
    assert!(swept.tasks > 0, "no task was read at all");
    assert_eq!(swept.deleted, 0);
    assert!(
        !swept.left(),
        "tasks or folders that can't be read: {:?}",
        swept.why
    );
}

#[test]
fn task_paths_compare_as_windows_compares_them_not_by_unicode_lowercase() {
    // CompareStringOrdinal, case ignored: case doesn't matter, but the Kelvin sign (U+212A),
    // which Unicode lowercases to `k`, is a character of its own.
    assert_eq!(same_task_path(r"\A\Task", r"\a\TASK"), Ok(true));
    assert_eq!(same_task_path("\\A\\Tas\u{212A}", r"\A\Task"), Ok(false));
    assert_eq!(
        same_task_path("\\A\\TAS\u{212A}", "\\a\\tas\u{212A}"),
        Ok(true)
    );
    assert_eq!(same_task_path(r"\A\Task", r"\A\Tasks"), Ok(false));
}

/// A task as the test user registers it from XML: its own, on a trigger far ahead, with `sddl`
/// as its descriptor; UTF-16LE after its byte order mark, as `schtasks /xml` reads it.
fn task_xml(user: &str, sddl: &str) -> Vec<u8> {
    let principal = format!("<UserId>{user}</UserId><LogonType>InteractiveToken</LogonType>");
    task_xml_for(&principal, sddl)
}

/// [`task_xml`] with `principal` as what its `Principal` element holds.
fn task_xml_for(principal: &str, sddl: &str) -> Vec<u8> {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n<Task version=\"1.2\" \
         xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\r\n  \
         <RegistrationInfo><SecurityDescriptor>{sddl}</SecurityDescriptor></RegistrationInfo>\
         \r\n  <Triggers><TimeTrigger><StartBoundary>2099-01-01T00:00:00</StartBoundary>\
         </TimeTrigger></Triggers>\r\n  <Principals><Principal id=\"Author\">{principal}\
         </Principal></Principals>\r\n  \
         <Actions Context=\"Author\"><Exec><Command>{}\\cmd.exe</Command><Arguments>/c exit 0\
         </Arguments></Exec></Actions>\r\n</Task>\r\n",
        system32()
    );
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
#[expect(
    clippy::disallowed_methods,
    reason = "writes the test user's task XML into the test's own folder"
)]
fn elevated_a_task_whose_descriptor_shuts_system_out_is_deleted_or_56_never_left_at_0() {
    if !gate("elevated_a_task_whose_descriptor_shuts_system_out_is_deleted_or_56_never_left_at_0") {
        return;
    }
    // 1. A standard account, as the test above makes one.
    clear_leftovers(TASK_TEST_PREFIX);
    let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let sid = user.sid.clone();
    eprintln!("1. the test user: {} {}", user.name, sid.as_str());
    let dir = TempDir::new("sweep-hidden");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();
    let name = format!("ClauseoSweepHidden{}{}", std::process::id(), random_hex(2));
    let schtasks = format!(r"{}\schtasks.exe", system32());

    // 2. As that user: a task whose descriptor denies SYSTEM, and Administrators (this process
    // is an elevated administrator standing in for SYSTEM, which runs the real sweep), all
    // access. A Task Scheduler that lists only what its caller may read leaves it out.
    let sddl = format!(
        "O:{0}D:P(D;;FA;;;SY)(D;;FA;;;BA)(A;;FA;;;{0})",
        sid.as_str()
    );
    let xml = dir.path().join("hidden.xml");
    std::fs::write(&xml, task_xml(sid.as_str(), &sddl)).unwrap();
    let create = format!(
        r#""{schtasks}" /create /tn {name} /xml "{}" /f"#,
        xml.display()
    );
    let (code, printed) = try_in_cmd(&user, dir.path(), &create);
    eprintln!("2. as the test user: {create}\nexit {code:?}\n{printed}");
    // Should Windows refuse such a descriptor from a standard user, the gap can't arise: say so
    // in 0049 ("Sweeping the pool's tasks and BITS jobs") and drop this test.
    assert_eq!(
        code,
        Some(0),
        "schtasks /create /xml as the test user:\n{printed}"
    );

    // 3. Whether the Task Scheduler's listing gives it to this process, then the walk and the
    // cross-check, the test user the pool.
    let mut sweeper = WindowsSweeper::new();
    let root = sweeper.root().unwrap();
    let listed = sweeper
        .tasks(&root)
        .unwrap()
        .iter()
        .any(|task| task.name == name);
    eprintln!("3. listed by GetTasks: {listed}");
    let swept = sweep_tasks(&mut sweeper, std::slice::from_ref(&sid));
    eprintln!(
        "   {} deleted, {} unreadable, {} not deleted, {} hidden, {} unlisted",
        swept.deleted, swept.unreadable, swept.not_deleted, swept.hidden, swept.unlisted
    );
    for (why, count) in &swept.why {
        eprintln!("{count} x {why}");
    }

    // 4. As the test user, who may read its own task: is it still there?
    let query = format!(r#""{schtasks}" /query /tn {name}"#);
    let (code, printed) = try_in_cmd(&user, dir.path(), &query);
    let left = code == Some(0);
    eprintln!("4. still there: {left}\n{printed}");
    if left {
        let delete = format!(r#""{schtasks}" /delete /tn {name} /f"#);
        let (code, printed) = try_in_cmd(&user, dir.path(), &delete);
        eprintln!("the clean-up, as the test user: exit {code:?}\n{printed}");
    }
    assert!(
        !left || swept.left(),
        "the task was left and the sweep would have ended 0: {swept:?}"
    );
    // Missing from the listing, only the cross-check could have caught it: this task, by its
    // path, among the hidden ones, not just any hidden task.
    let path = format!(r"\{name}");
    let caught = swept
        .hidden_paths
        .iter()
        .any(|hidden| same_task_path(hidden, &path) == Ok(true));
    assert!(
        listed || caught,
        "the listing hid the task and the cross-check didn't find it: {swept:?}"
    );
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
#[expect(
    clippy::disallowed_methods,
    reason = "writes the test user's task XML into the test's own folder"
)]
fn elevated_a_standard_user_cant_register_a_task_owned_by_system_or_administrators() {
    if !gate("elevated_a_standard_user_cant_register_a_task_owned_by_system_or_administrators") {
        return;
    }
    // Both owner checks (the task's own descriptor, its cache key's `SD`) rest on this: a
    // standard user's task whose descriptor names SYSTEM or Administrators as its owner is
    // refused, or registered owned by that user, never by the owner it named.
    clear_leftovers(TASK_TEST_PREFIX);
    let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let sid = user.sid.clone();
    eprintln!("1. the test user: {} {}", user.name, sid.as_str());
    let dir = TempDir::new("sweep-owner");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();
    let suffix = format!("{}{}", std::process::id(), random_hex(2));
    // The owner each descriptor names (none: ownerless), and a task whose own descriptor names
    // the test user, whose descriptor the test user then tries to rewrite.
    let names = [
        ("O:SY", format!("ClauseoSweepOwnerSy{suffix}")),
        ("O:BA", format!("ClauseoSweepOwnerBa{suffix}")),
        ("", format!("ClauseoSweepOwnerNone{suffix}")),
        ("O:self", format!("ClauseoSweepOwnerSet{suffix}")),
    ];
    // Dropped before the user, so the account outlives its tasks' clean-up.
    let _made = Made(names.iter().map(|(_, name)| name.clone()).collect());
    let schtasks = format!(r"{}\schtasks.exe", system32());
    let mut sweeper = WindowsSweeper::new();
    for (alias, name) in &names {
        // 2. As that user: a task whose descriptor names `alias` its owner, readable by all
        // three, so this process can read what owner it got.
        let owner = alias.replace("self", sid.as_str());
        let sddl = format!(
            "{owner}D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;{})",
            sid.as_str()
        );
        let xml = dir.path().join(format!("{name}.xml"));
        std::fs::write(&xml, task_xml(sid.as_str(), &sddl)).unwrap();
        let create = format!(
            r#""{schtasks}" /create /tn {name} /xml "{}" /f"#,
            xml.display()
        );
        let (code, printed) = try_in_cmd(&user, dir.path(), &create);
        eprintln!("2. {alias:?} as the test user: exit {code:?}\n{printed}");
        if code != Some(0) {
            assert_ne!(*alias, "O:self", "the test user's own task:\n{printed}");
            eprintln!("   refused: the gap can't arise for {alias:?}");
            continue;
        }
        // 3. Registered: its owner, from the cache key's `SD` and from the Task Scheduler,
        // must be the test user.
        let cached = sweeper.task_cache_owner(&format!(r"\{name}"));
        eprintln!("3. its cache key's owner: {cached:?}");
        assert_eq!(cached.as_ref(), Ok(&sid), "{alias:?}: the cache key's SD");
        let root = sweeper.root().unwrap();
        let read = sweeper.task_sddl(&root, name).unwrap();
        let owner = sddl_owner(&read);
        eprintln!("   its descriptor's owner: {owner:?}");
        assert_eq!(
            owner,
            Ok(Some(sid.clone())),
            "{alias:?}: the task's descriptor"
        );
    }
    // 4. As that user, on its own task: IRegisteredTask::SetSecurityDescriptor naming SYSTEM,
    // then Administrators, its owner. Expected refused (an owner can be set only to the
    // caller or a group its token may own by); either way the owner the cache records stays
    // the test user's.
    let (_, name) = &names[3];
    for alias in ["SY", "BA"] {
        let sddl = format!(
            "O:{alias}D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;{})",
            sid.as_str()
        );
        // Anything before the call that fails (PowerShell, the COM connect, the task's lookup)
        // ends it 1 and fails the test; the call itself ends it 0 when it worked, 3 when refused,
        // each saying so (what it printed holds stderr too).
        let script = format!(
            "$ErrorActionPreference='Stop'; $s=New-Object -ComObject Schedule.Service; \
             $s.Connect(); $t=$s.GetFolder('\\').GetTask('{name}'); \
             try {{ $t.SetSecurityDescriptor('{sddl}', 0); 'SET-OK'; exit 0 }} \
             catch {{ 'SET-REFUSED: ' + $_.Exception.Message; exit 3 }}"
        );
        let (code, printed) = try_in_cmd(&user, dir.path(), &powershell(&script));
        eprintln!("4. SetSecurityDescriptor O:{alias} as the test user: exit {code:?}\n{printed}");
        let ran = match code {
            Some(0) => printed.contains("SET-OK"),
            Some(3) => printed.contains("SET-REFUSED"),
            _ => false,
        };
        assert!(
            ran,
            "O:{alias}: PowerShell didn't reach the call: exit {code:?}\n{printed}"
        );
        let cached = sweeper.task_cache_owner(&format!(r"\{name}"));
        eprintln!("   its cache key's owner: {cached:?}");
        assert_eq!(
            cached.as_ref(),
            Ok(&sid),
            "O:{alias} set later: the cache key's SD"
        );
    }
}

/// A cmd line that runs `script` in Windows PowerShell, passed encoded (UTF-16LE, base64) so
/// cmd reads none of its characters.
fn powershell(script: &str) -> String {
    use base64::Engine as _;
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let exe = format!(r"{}\WindowsPowerShell\v1.0\powershell.exe", system32());
    format!(r#""{exe}" -NoProfile -NonInteractive -EncodedCommand {encoded}"#)
}

#[test]
fn a_task_cache_id_is_a_guid_in_braces_and_nothing_else() {
    assert!(is_guid("{0A1B2C3D-4E5F-6789-ABCD-EF0123456789}"));
    for not in [
        "0A1B2C3D-4E5F-6789-ABCD-EF0123456789",
        "{0A1B2C3D-4E5F-6789-ABCD-EF012345678}",
        "{0A1B2C3D-4E5F-6789-ABCD-EF0123456789}\\x",
        "{..\\..\\Tree-4E5F-6789-ABCD-EF0123456789}",
        "{0A1B2C3D-4E5F-6789-ABCD-EF012345678G}",
        "",
    ] {
        assert!(!is_guid(not), "{not}");
    }
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
#[expect(
    clippy::disallowed_methods,
    reason = "writes the test user's task XML into the test's own folder"
)]
fn elevated_a_standard_user_cant_register_a_task_for_another_principal() {
    if !gate("elevated_a_standard_user_cant_register_a_task_for_another_principal") {
        return;
    }
    // The file-and-cache rule trusts a task file naming someone else because a standard user
    // can't register a task for another principal without that principal's credentials: one
    // for another local user (no password, an interactive token) or for Users must fail.
    clear_leftovers(TASK_TEST_PREFIX);
    let user = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let other = TestUser::named(TASK_TEST_PREFIX, &random_hex(3));
    let sid = user.sid.clone();
    eprintln!(
        "1. the test user {}, the other user {}",
        user.name, other.name
    );
    let dir = TempDir::new("sweep-principal");
    let grant = format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{})",
        sid.as_str()
    );
    protect_with(dir.path(), &grant).unwrap();
    let suffix = format!("{}{}", std::process::id(), random_hex(2));
    let sddl = format!(
        "O:{0}D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;{0})",
        sid.as_str()
    );
    let token = "<LogonType>InteractiveToken</LogonType>";
    // Each case: its name, what its Principal holds, its logon type
    // (TASK_LOGON_INTERACTIVE_TOKEN 3, TASK_LOGON_GROUP 4), and whether it must register. The
    // first, the test user's own, shows a refusal below isn't the script's or the XML's.
    let own = format!("<UserId>{}</UserId>{token}", sid.as_str());
    let others = format!("<UserId>{}</UserId>{token}", other.sid.as_str());
    let users = "<GroupId>S-1-5-32-545</GroupId>".to_owned();
    let cases = [
        ("Own", own, 3, true),
        ("Other", others, 3, false),
        ("Users", users, 4, false),
    ];
    let names: Vec<String> = cases
        .iter()
        .map(|(case, ..)| format!("ClauseoSweepPrincipal{case}{suffix}"))
        .collect();
    // Dropped before the users, so the accounts outlive their tasks' clean-up.
    let _made = Made(names.clone());
    for ((case, principal, logon, registers), name) in cases.iter().zip(&names) {
        let xml = dir.path().join(format!("{case}.xml"));
        std::fs::write(&xml, task_xml_for(principal, &sddl)).unwrap();
        let script = format!(
            "$ErrorActionPreference='Stop'; $x=Get-Content -Raw -LiteralPath '{}'; \
             $s=New-Object -ComObject Schedule.Service; $s.Connect(); $f=$s.GetFolder('\\'); \
             try {{ $null=$f.RegisterTask('{name}', $x, 6, $null, $null, {logon}); 'REG-OK'; \
             exit 0 }} catch {{ 'REG-REFUSED: ' + $_.Exception.Message; exit 3 }}",
            xml.display()
        );
        let (code, printed) = try_in_cmd(&user, dir.path(), &powershell(&script));
        eprintln!("2. {case} as the test user: exit {code:?}\n{printed}");
        let (exit, said) = if *registers {
            (Some(0), "REG-OK")
        } else {
            (Some(3), "REG-REFUSED")
        };
        assert!(
            code == exit && printed.contains(said),
            "{case}: expected exit {exit:?} with {said}:\n{printed}"
        );
    }
}
