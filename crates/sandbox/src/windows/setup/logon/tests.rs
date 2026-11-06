//! The set-up's Windows calls on a real machine (S15 `PR4f`): the logon, the profile, the
//! preflight, the process list and the journal.
//!
//! The unelevated tests run on every Windows CI run. The `elevated_` tests are `#[ignore]` and
//! run only from CI's elevated step: each skips unless this process's token is elevated and
//! `CLAUSEO_ELEVATED_TESTS=1`, so they never touch a dev box. Each makes its account under a
//! name unique to the run ([`PREFIX`] and six hex digits), and first clears whatever a killed
//! earlier try of this family left (accounts named `ClauseoSbTL*`, and profiles of such names).
//! Other test families use other prefixes, so this one never clears their accounts.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::super::journal::{journal_size, journal_volumes};
use super::super::machine::Password;
use super::super::preflight::{logon_right, machine_domain, seclogon};
use super::super::processes::{Stopped, running, stop_pool_processes};
use super::super::profile::{delete_profile, profile_of};
use super::log_on_once;
use crate::windows::acl::object::Handle;
use crate::windows::record::current_user_sid;
use crate::windows::sids::Sid;

/// This family's account name prefix (`PR4f`'s logon tests): `ClauseoSbTL`, then six hex
/// digits, 17 characters in all, under the 20 that local account names allow.
const PREFIX: &str = "ClauseoSbTL";

/// Whether this process's token is elevated. A token that cannot be asked counts as not.
// TODO(S15 PR4e): use the shared helper PR4e moves into `test_acl.rs` once it merges.
#[expect(unsafe_code, reason = "FFI to read this process's token elevation")]
fn elevated() -> bool {
    let mut token = HANDLE::default();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `token` is a live
    // out-pointer, owned by `Handle` once written.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }.is_err() {
        return false;
    }
    let token = Handle(token);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut needed = 0u32;
    let len = u32::try_from(size_of::<TOKEN_ELEVATION>()).unwrap_or_default();
    // SAFETY: `elevation` is live and `len` bytes long for the call, which writes a
    // TOKEN_ELEVATION into it; the handle came from OpenProcessToken.
    let asked = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            len,
            &raw mut needed,
        )
    };
    asked.is_ok() && elevation.TokenIsElevated != 0
}

/// Whether an elevated test may run here: an elevated token and `CLAUSEO_ELEVATED_TESTS=1`.
///
/// The elevated tests assume they run one at a time, serial via `PR4e`'s nextest test group for
/// `elevated_` tests: [`clear_leftovers`] deletes this family's accounts, so two of its tests
/// running at once would delete each other's.
fn gate(test: &str) -> bool {
    let asked = std::env::var("CLAUSEO_ELEVATED_TESTS").is_ok_and(|value| value == "1");
    if asked && elevated() {
        return true;
    }
    eprintln!("skipped {test}: needs an elevated token and CLAUSEO_ELEVATED_TESTS=1");
    false
}

/// `bytes` random bytes as lowercase hex.
fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).unwrap();
    buffer.iter().fold(String::new(), |mut text, byte| {
        write!(text, "{byte:02x}").unwrap();
        text
    })
}

/// Runs PowerShell's `script`; its standard output, trimmed. Panics when it fails (scripts
/// here carry no password).
#[expect(
    clippy::disallowed_methods,
    reason = "the tests read accounts with PowerShell"
)]
fn powershell(script: &str) -> String {
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .unwrap();
    assert!(output.status.success(), "PowerShell failed: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// The SID of the local account `name`.
fn sid_of(name: &str) -> Sid {
    Sid::parse(&powershell(&format!(
        "(Get-LocalUser -Name '{name}').SID.Value"
    )))
    .unwrap()
}

/// Deletes the account `name` (`net user /delete`); a failure is printed, not raised.
#[expect(
    clippy::disallowed_methods,
    reason = "the tests delete accounts with `net user`"
)]
fn delete_account(name: &str) {
    let output = Command::new("net")
        .args(["user", name, "/delete"])
        .output()
        .unwrap();
    if !output.status.success() {
        eprintln!("net user {name} /delete failed: {}", output.status);
    }
}

/// The owner the stop and the delete are given: this process's user, as in a set-up that
/// isn't over the shoulder.
fn me() -> Sid {
    current_user_sid().unwrap()
}

/// Clears what a killed earlier try of this family left: every `ClauseoSbTL*` account with
/// its profile, then any profile whose folder has that prefix and whose account is gone.
fn clear_leftovers() {
    let accounts = powershell(&format!(
        "Get-LocalUser -Name '{PREFIX}*' | ForEach-Object {{ $_.Name + ' ' + $_.SID.Value }}"
    ));
    for line in accounts.lines() {
        let Some((name, sid)) = line.trim().split_once(' ') else {
            continue;
        };
        if let Ok(sid) = Sid::parse(sid) {
            drop(delete_profile(&sid, &me()));
        }
        delete_account(name);
    }
    let profiles = powershell(&format!(
        "Get-ChildItem 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList' | \
         Where-Object {{ (Get-ItemProperty $_.PSPath).ProfileImagePath -like '*\\{PREFIX}*' }} | \
         ForEach-Object {{ $_.PSChildName }}"
    ));
    for line in profiles.lines() {
        if let Ok(sid) = Sid::parse(line.trim()) {
            drop(delete_profile(&sid, &me()));
        }
    }
}

/// A test account, made with a random password, deleted with its profile on drop.
struct TestUser {
    name: String,
    password: Password,
    sid: Sid,
}

impl TestUser {
    /// Makes `ClauseoSbTL<6 hex>` in `Users`, with a random password meeting the complexity
    /// policy. The password reaches PowerShell on its standard input, never a command line.
    fn new() -> TestUser {
        TestUser::named(&random_hex(3))
    }

    /// Makes `ClauseoSbTL<suffix>` as [`TestUser::new`] does.
    #[expect(
        clippy::disallowed_methods,
        reason = "the tests make accounts with PowerShell"
    )]
    fn named(suffix: &str) -> TestUser {
        let name = format!("{PREFIX}{suffix}");
        assert!(name.len() <= 20, "{name}");
        let password = Password::new(format!("Aa1!{}", random_hex(12)));
        let script = format!(
            "$p = [Console]::In.ReadLine() | ConvertTo-SecureString -AsPlainText -Force; \
             New-LocalUser -Name '{name}' -Password $p -AccountNeverExpires | Out-Null; \
             Add-LocalGroupMember -SID 'S-1-5-32-545' -Member '{name}'"
        );
        let mut child = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(password.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        drop(stdin);
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "New-LocalUser {name} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let sid = sid_of(&name);
        TestUser {
            name,
            password,
            sid,
        }
    }
}

impl Drop for TestUser {
    fn drop(&mut self) {
        if let Err(error) = delete_profile(&self.sid, &me()) {
            eprintln!("{}'s profile wasn't deleted: {error}", self.name);
        }
        delete_account(&self.name);
    }
}

#[test]
fn the_secondary_logon_query_answers() {
    // Hosted runners leave seclogon at its default (manual); any answer is a working query.
    let start = seclogon().unwrap();
    eprintln!("seclogon starts: {start:?}");
}

#[test]
fn the_process_list_reads_this_process_and_its_user() {
    let own = std::process::id();
    let me = current_user_sid().unwrap();
    let seen = running().unwrap();
    let mine = seen.iter().find(|process| process.pid == own).unwrap();
    assert_eq!(mine.user.as_ref(), Some(&me));
}

#[test]
fn stopping_refuses_sids_that_cant_be_pool_users_and_stops_nothing_for_an_unused_one() {
    // Another domain's SID, a well-known SID, and this process's user are refused outright.
    let own = current_user_sid().unwrap();
    for refused in [
        Sid::parse("S-1-5-21-1-2-3-4242").unwrap(),
        Sid::parse("S-1-5-18").unwrap(),
        own,
    ] {
        let error = stop_pool_processes(std::slice::from_ref(&refused), &me()).unwrap_err();
        assert!(error.to_string().contains(refused.as_str()), "{error}");
        assert!(delete_profile(&refused, &me()).is_err());
    }
    // An unused account SID of this machine resolves to no account, so it passes: nothing runs
    // as it, so nothing is stopped.
    let [a, b, c] = machine_domain().unwrap();
    let unused = Sid::parse(&format!("S-1-5-21-{a}-{b}-{c}-4000000000")).unwrap();
    let stopped = stop_pool_processes(std::slice::from_ref(&unused), &me()).unwrap();
    assert_eq!(stopped, Stopped::default());
    assert_eq!(stop_pool_processes(&[], &me()).unwrap(), Stopped::default());
    // The same SID as the owner (an over-the-shoulder run, where the owner isn't this
    // process's user) is refused.
    let error = stop_pool_processes(std::slice::from_ref(&unused), &unused).unwrap_err();
    assert!(error.to_string().contains("owner"), "{error}");
    assert!(delete_profile(&unused, &unused).is_err());
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_an_account_not_named_as_a_pool_user_is_refused() {
    if !gate("elevated_an_account_not_named_as_a_pool_user_is_refused") {
        return;
    }
    clear_leftovers();
    // `ClauseoSbTL`, five hex digits and a `g`: this family's prefix, so a leftover is
    // cleared, but not a name the check takes as a pool user's.
    let mut suffix = random_hex(3);
    suffix.replace_range(5.., "g");
    let other = TestUser::named(&suffix);
    let error = stop_pool_processes(std::slice::from_ref(&other.sid), &me()).unwrap_err();
    assert!(error.to_string().contains("isn't a pool user"), "{error}");
    assert!(delete_profile(&other.sid, &me()).is_err());
    // A name of the family's form passes.
    let user = TestUser::new();
    let stopped = stop_pool_processes(std::slice::from_ref(&user.sid), &me()).unwrap();
    assert_eq!(stopped, Stopped::default());
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_log_on_once_makes_a_profile_and_delete_profile_removes_it() {
    if !gate("elevated_log_on_once_makes_a_profile_and_delete_profile_removes_it") {
        return;
    }
    clear_leftovers();
    let user = TestUser::new();
    let profile = log_on_once(&user.name, &user.password).unwrap();
    assert!(!profile.temporary, "{profile:?}");
    let local_low = profile.path.join("AppData").join("LocalLow");
    assert!(local_low.is_dir(), "{}", local_low.display());
    // The profile service makes the folders. Their owner is SYSTEM, the user, or the account
    // that loaded the profile (the elevated set-up's; on the hosted runner, which copies a
    // Default profile its own account built, the built-in Administrator). Never another user.
    let owner = crate::windows::acl::owner(&local_low).unwrap();
    eprintln!("LocalLow's owner: {}", owner.as_str());
    assert!(
        owner.as_str() == "S-1-5-18" || owner == user.sid || owner == me(),
        "{}",
        owner.as_str()
    );
    let listed = profile_of(&user.sid).unwrap().unwrap();
    assert_eq!(listed.path, profile.path);
    assert!(!listed.temporary);
    // A second logon finds the same profile.
    assert_eq!(log_on_once(&user.name, &user.password).unwrap(), profile);

    delete_profile(&user.sid, &me()).unwrap();
    assert!(
        !Path::new(&profile.path).exists(),
        "{}",
        profile.path.display()
    );
    assert_eq!(profile_of(&user.sid).unwrap(), None);
    // Deleting what is gone is `Ok`, as a re-run needs.
    delete_profile(&user.sid, &me()).unwrap();
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_wrong_password_is_refused_with_its_code_alone() {
    if !gate("elevated_a_wrong_password_is_refused_with_its_code_alone") {
        return;
    }
    clear_leftovers();
    let user = TestUser::new();
    let wrong = Password::new(format!("Bb2@{}", random_hex(12)));
    let error = log_on_once(&user.name, &wrong).unwrap_err();
    let text = format!("{error} {error:?}");
    assert!(
        text.contains("LogonUserW") && text.contains("1326"),
        "{text}"
    );
    assert!(!text.contains(wrong.as_str()) && !text.contains(user.password.as_str()));
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_the_logon_right_and_the_journal_answer() {
    if !gate("elevated_the_logon_right_and_the_journal_answer") {
        return;
    }
    let right = logon_right().unwrap();
    eprintln!("Deny log on locally: {right:?}");
    let volumes = journal_volumes(&[]).unwrap();
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    let system = volumes
        .iter()
        .find(|volume| {
            volume
                .to_string_lossy()
                .eq_ignore_ascii_case(&format!(r"{drive}\"))
        })
        .unwrap_or_else(|| panic!("{volumes:?}"));
    // Read only: the runner's journal is never grown here.
    let size = journal_size(system).unwrap();
    eprintln!("{}'s journal: {size:?}", system.display());
}
