//! No keychain in the jail (0038): a login-keychain item stored outside, readable there by the
//! program that stored it, is not readable inside, no item is listed, and Claude Code's own
//! lookup ends as "not found", which sends it to its `.credentials.json`.

use std::path::{Path, PathBuf};

use crate::support::{Output, World, launcher, run, run_outside};

/// The service of the throwaway generic password.
const SERVICE: &str = "clauseo-jail-test";

/// The host of the throwaway git credential.
const GIT_HOST: &str = "clauseo-jail-test.invalid";

/// The `PATH` the jail's programs get.
const PATH: &str = "/usr/bin:/bin";

/// `security`'s exit code for an item not found (`errSecItemNotFound`, -25300, low byte).
const NOT_FOUND: i32 = 44;

/// The real home: the keychain search list and the login keychain are found through `HOME`, and
/// the policy's home names the keychain folder the profile denies.
fn real_home() -> String {
    std::env::var("HOME").expect("HOME")
}

/// `/bin/sh -c script` outside the jail, with the test's own environment.
fn sh_outside(script: &str) -> Output {
    run_outside("/bin/sh", &["-c", script], Path::new("/"))
}

/// `git-credential-osxkeychain`'s path, in git's exec path.
fn osxkeychain() -> PathBuf {
    let out = run_outside("/usr/bin/git", &["--exec-path"], Path::new("/"));
    PathBuf::from(out.assert_ok().stdout.trim()).join("git-credential-osxkeychain")
}

/// The throwaway generic password and git credential, both under `account`, deleted on drop
/// even when an assertion failed first.
struct Items {
    account: String,
    helper: PathBuf,
}

impl Items {
    fn store(account: &str, secret: &str) -> Items {
        let items = Items {
            account: account.to_owned(),
            helper: osxkeychain(),
        };
        run_outside(
            "/usr/bin/security",
            &[
                "add-generic-password",
                "-s",
                SERVICE,
                "-a",
                account,
                "-w",
                secret,
            ],
            Path::new("/"),
        )
        .assert_ok();
        sh_outside(&format!(
            "{} store",
            items.git_input(&format!("password={secret}\n"))
        ))
        .assert_ok();
        items
    }

    /// `printf '<the credential's fields>' | <helper>`, its action still to add.
    fn git_input(&self, extra: &str) -> String {
        format!(
            "printf 'protocol=https\\nhost={GIT_HOST}\\nusername={}\\n{extra}\\n' | '{}'",
            self.account,
            self.helper.display()
        )
    }
}

impl Drop for Items {
    fn drop(&mut self) {
        let deleted = run_outside(
            "/usr/bin/security",
            &[
                "delete-generic-password",
                "-s",
                SERVICE,
                "-a",
                &self.account,
            ],
            Path::new("/"),
        );
        if deleted.code != Some(0) {
            eprintln!("deleting the {SERVICE} item: {deleted:#?}");
        }
        let erased = sh_outside(&format!("{} erase", self.git_input("")));
        if erased.code != Some(0) {
            eprintln!("erasing the {GIT_HOST} credential: {erased:#?}");
        }
    }
}

#[test]
fn keychain_items_stored_outside_are_not_read_or_listed_inside() {
    let w = World::new();
    let home = real_home();
    let account = format!("csb-{}", std::process::id());
    let secret = format!("jail-secret-{}", std::process::id());
    let items = Items::store(&account, &secret);
    let find = ["find-generic-password", "-s", SERVICE, "-a", &account, "-w"];
    let get = format!("{} get", items.git_input(""));
    // Outside, the programs that stored them read them back: the checks below are not vacuous.
    let outside = run_outside("/usr/bin/security", &find, Path::new("/"));
    assert_eq!(outside.assert_ok().stdout.trim(), secret);
    assert!(sh_outside(&get).assert_ok().stdout.contains(&secret));

    let policy = w.builder().home(&home).build().unwrap();
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let env = [("PATH", PATH), ("HOME", home.as_str())];
    let no_secret = |what: &str, out: &Output| {
        assert_ne!(out.code, Some(0), "{what}: {out:#?}");
        assert!(!out.stdout.contains(&secret), "{what}: {out:#?}");
        assert!(!out.stderr.contains(&secret), "{what}: {out:#?}");
    };

    let found = run(&launcher, "/usr/bin/security", &find, &m1, &env);
    no_secret("find-generic-password", &found);
    let got = run(&launcher, "/bin/sh", &["-c", &get], &m1, &env);
    no_secret("git-credential-osxkeychain get", &got);
    assert!(got.stdout.is_empty(), "{got:#?}");

    let dump = run(
        &launcher,
        "/usr/bin/security",
        &["dump-keychain"],
        &m1,
        &env,
    );
    assert_ne!(dump.code, Some(0), "{dump:#?}");
    assert!(dump.stdout.is_empty(), "{dump:#?}");
    assert!(!dump.stderr.contains(SERVICE), "{dump:#?}");
    drop(items);
}

/// Claude Code reads its login with this command and takes exit 44 ("not found") as no item,
/// then reads `.credentials.json`; any other code is a read failure and skips that fallback.
/// Measured 2026-11-09: 44, and nothing on stdout.
#[test]
fn claude_codes_keychain_lookup_ends_as_not_found() {
    let w = World::new();
    let home = real_home();
    let user = std::env::var("USER").expect("USER");
    let policy = w.builder().home(&home).build().unwrap();
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let env = [
        ("PATH", PATH),
        ("HOME", home.as_str()),
        ("USER", user.as_str()),
    ];
    let out = run(
        &launcher,
        "/usr/bin/security",
        &[
            "find-generic-password",
            "-a",
            &user,
            "-s",
            "Claude Code-credentials",
            "-w",
        ],
        &m1,
        &env,
    );
    assert_eq!(out.code, Some(NOT_FOUND), "{out:#?}");
    assert!(out.stdout.is_empty(), "{out:#?}");
    assert!(
        out.stderr
            .contains("The specified item could not be found in the keychain."),
        "{out:#?}"
    );
}

/// Stage D's `security` shim (`Clauseo.app/Contents/Resources/keychain-shim/security`), in
/// short: past any leading options (`-q`, `-v`, `-i`, ...), a read (`find-*`, `dump-keychain`)
/// is "not found" (exit 44), so Claude Code reads its `.credentials.json`; anything else fails at
/// once with a fixed line. It never reads stdin.
const SHIM: &str = r#"#!/bin/sh
while [ "$#" -gt 0 ]; do
  case "$1" in
    -*) shift ;;
    *) break ;;
  esac
done
case "${1-}" in
  find-*|dump-keychain)
    echo 'security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain.' >&2
    exit 44
    ;;
esac
echo 'security: not available in Clauseo' >&2
exit 1
"#;

/// Perl: an alarm in one second, then `exec` of its arguments. The alarm outlives the exec, so a
/// program that hangs is killed by SIGALRM and has no exit code.
const ALARM: &str = r#"alarm 1; exec @ARGV or die "exec: $!\n""#;

/// The shim in a runtime path, first on `PATH`: the jail runs it from there, its lookup is
/// "not found" and its writes fail at once, and `/usr/bin/security` by its full path is still
/// refused by the keychain's denies.
#[test]
fn a_security_shim_in_a_runtime_path_answers_not_found() {
    let w = World::new();
    let shim = w.file("keychain-shim/security", SHIM);
    run_outside(
        "/bin/chmod",
        &["755", shim.to_str().unwrap()],
        Path::new("/"),
    )
    .assert_ok();
    let home = real_home();
    let user = std::env::var("USER").expect("USER");
    let policy = w
        .builder()
        .home(&home)
        .runtime(w.at("keychain-shim"))
        .build()
        .unwrap();
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let path = format!("{}:{PATH}", w.at("keychain-shim").display());
    let env = [
        ("PATH", path.as_str()),
        ("HOME", home.as_str()),
        ("USER", user.as_str()),
    ];
    // Under a one-second alarm that survives the exec: a hang ends in SIGALRM, not an exit code.
    let timed = |script: &str| {
        run(
            &launcher,
            "/usr/bin/perl",
            &["-e", ALARM, "/bin/sh", "-c", script],
            &m1,
            &env,
        )
    };

    let which = timed("command -v security");
    assert_eq!(which.assert_ok().stdout.trim(), shim.to_str().unwrap());

    let found =
        timed("security find-generic-password -a \"$USER\" -w -s 'Claude Code-credentials'");
    assert_eq!(found.code, Some(NOT_FOUND), "{found:#?}");
    assert!(found.stdout.is_empty(), "{found:#?}");
    assert_eq!(
        found.stderr,
        "security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain.\n"
    );

    // Past leading options too.
    for script in [
        "security -q find-generic-password -a \"$USER\" -w -s 'Claude Code-credentials'",
        "security -v -q find-internet-password -s example.com",
        "security -i dump-keychain",
    ] {
        let out = timed(script);
        assert_eq!(out.code, Some(NOT_FOUND), "{script}: {out:#?}");
        assert!(out.stdout.is_empty(), "{script}: {out:#?}");
    }

    let added = timed("security add-generic-password -a \"$USER\" -s x -w y");
    assert_eq!(added.code, Some(1), "{added:#?}");
    assert_eq!(added.stderr, "security: not available in Clauseo\n");

    // Interactive mode with commands on stdin: refused at once, stdin unread.
    let interactive = timed(
        "printf 'find-generic-password -s x\\nadd-generic-password -s x -w y\\n' | security -i",
    );
    assert_eq!(interactive.code, Some(1), "{interactive:#?}");
    assert!(interactive.stdout.is_empty(), "{interactive:#?}");

    let real = timed(
        "/usr/bin/security find-generic-password -a \"$USER\" -w -s 'Claude Code-credentials'",
    );
    assert_eq!(real.code, Some(NOT_FOUND), "{real:#?}");
    assert!(real.stdout.is_empty(), "{real:#?}");
    assert!(
        real.stderr
            .contains("The specified item could not be found in the keychain."),
        "{real:#?}"
    );
}
