//! A throwaway spike for S15 PR7's review item 1, run unelevated nowhere: on GitHub's hosted
//! Windows runners (elevated, with BFE), via `.github/workflows/exp-wfp-denyonly.yml`.
//!
//! It makes a throwaway local user and group, installs tier B's real filters for them
//! (`wfp::install` in a test namespace: blocks that deny the group and the user, matched
//! `NOT_EQUAL`, and the user's static permit for 127.0.0.1 TCP 61080-61179), removes the
//! firewall rules so only WFP is measured, then starts a probe under each token and tries a
//! loopback listener outside the range and one inside it. One line per row:
//!
//! ```text
//! ROW <n> <token> <target> expect=<reached|blocked|measured> got=<...> PASS|FAIL|INFO
//! ```
//!
//! then `SUMMARY pass=<n> fail=<n> info=<n>`. It removes everything it made on every exit path,
//! by exact names and keys. Off Windows it says so.

#[cfg(windows)]
#[path = "s15_wfp_vm/derived.rs"]
mod derived;
#[cfg(windows)]
#[path = "s15_wfp_vm/probe.rs"]
mod probe;
#[cfg(windows)]
#[path = "wfp_denyonly_spike/tokens.rs"]
mod tokens;

use std::io::Write;

fn say(line: &str) {
    let mut out = std::io::stdout();
    drop(writeln!(out, "{line}").and_then(|()| out.flush()));
}

#[cfg(not(windows))]
fn main() {
    say("wfp_denyonly_spike runs on Windows only");
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = std::env::current_exe()?;
    if let Some(("probe", rest)) = args.split_first().map(|(c, r)| (c.as_str(), r)) {
        std::process::exit(i32::from_ne_bytes(probe::run(&exe, rest).to_ne_bytes()));
    }
    spike::run(&exe)
}

#[cfg(windows)]
mod spike {
    use std::net::TcpListener;
    use std::path::Path;

    use clauseo_sandbox::windows::sids::Sid;
    use clauseo_sandbox::windows::wfp::spec::rule_names;
    use clauseo_sandbox::windows::wfp::{
        Namespace, NetworkSetup, check_launch_in, install, remove,
    };

    use super::probe::{Got, step};
    use super::say;
    use super::tokens::{self, Derive, Token};

    /// The spike's own WFP names, apart from production and the crate's tests.
    fn ns() -> Namespace {
        Namespace::numbered_test(9)
    }

    /// Deletes what the spike made, on drop.
    struct Cleanup {
        user: String,
        group: String,
        owner: Option<Sid>,
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Err(error) = remove(&ns(), self.owner.as_ref()) {
                say(&format!("CLEANUP wfp: {error}"));
            }
            drop(tokens::net(&["user", &self.user, "/delete"]));
            drop(tokens::net(&["localgroup", &self.group, "/delete"]));
        }
    }

    /// A random hex tag.
    fn tag() -> String {
        let mut bytes = [0u8; 4];
        getrandom::fill(&mut bytes).unwrap_or_default();
        bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .concat()
    }

    pub(crate) fn run(exe: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let t = tag();
        let user = format!("S15Spk{t}");
        let group = format!("S15SpkG{t}");
        let password = format!("Aa1!{t}{}", tag());
        tokens::net(&["localgroup", &group, "/add"])?;
        let mut cleanup = Cleanup {
            user: user.clone(),
            group: group.clone(),
            owner: None,
        };
        tokens::net(&["user", &user, &password, "/add"])?;
        tokens::net(&["localgroup", &group, &user, "/add"])?;
        let (kind, pool) = tokens::logon(&user, &password)?;
        say(&format!("LOGON {kind}"));
        let user_sid = super::sids::user(&pool)?;
        let group_sid = super::sids::group(&pool, &group)?;
        let admin = tokens::own()?;
        let admin_sid = super::sids::user(&admin)?;
        let owner = Sid::parse(&admin_sid)?;
        cleanup.owner = Some(owner.clone());
        let setup = NetworkSetup::new(&owner, &Sid::parse(&group_sid)?, &[Sid::parse(&user_sid)?])?;
        let report = install(&ns(), &setup)?;
        say(&format!(
            "INSTALL filters={} firewall={:?}",
            report.filters, report.firewall
        ));
        for name in rule_names(&ns()) {
            tokens::netsh(&[
                "advfirewall",
                "firewall",
                "delete",
                "rule",
                &format!("name={name}"),
            ]);
        }
        let outside = TcpListener::bind("127.0.0.1:0")?;
        let inside = (61080..=61179)
            .find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok())
            .ok_or("no range port")?;
        let (o, i) = (outside.local_addr()?.port(), inside.local_addr()?.port());
        let accept = |l: TcpListener| {
            std::thread::spawn(move || {
                for s in l.incoming() {
                    drop(s);
                }
            })
        };
        drop((accept(outside), accept(inside)));
        let user_s = Sid::parse(&user_sid)?;
        let check_pool = check_launch_in(&ns(), &user_s, &[i]).map_err(|e| e.to_string());
        let check_admin = check_launch_in(&ns(), &owner, &[i]).map_err(|e| e.to_string());
        let world = World {
            pool,
            admin,
            user_sid,
            group_sid,
            admin_sid,
            o,
            i,
        };
        let tally = measure(&world, exe, &check_pool, &check_admin);
        say(&format!(
            "SUMMARY pass={} fail={} info={}",
            tally[0], tally[1], tally[2]
        ));
        say(if tally[1] == 0 {
            "SPIKE OK"
        } else {
            "SPIKE FINDINGS"
        });
        drop(cleanup);
        Ok(())
    }

    /// What the rows need.
    struct World {
        pool: Token,
        admin: Token,
        user_sid: String,
        group_sid: String,
        admin_sid: String,
        o: u16,
        i: u16,
    }

    /// Prints one row and counts it in `tally` (pass, fail, info).
    fn row(
        tally: &mut [usize; 3],
        n: u32,
        who: &str,
        target: &str,
        expect: &str,
        got: &str,
        pass: Option<bool>,
    ) {
        let (slot, verdict) = match pass {
            Some(true) => (0, "PASS"),
            Some(false) => (1, "FAIL"),
            None => (2, "INFO"),
        };
        if let Some(count) = tally.get_mut(slot) {
            *count += 1;
        }
        say(&format!(
            "ROW {n} {who} {target} expect={expect} got={got} {verdict}"
        ));
    }

    /// Every row; the pass, fail and info counts.
    fn measure(
        w: &World,
        exe: &Path,
        check_pool: &Result<(), String>,
        check_admin: &Result<(), String>,
    ) -> [usize; 3] {
        let mut tally = [0usize; 3];
        let t = &mut tally;
        row(
            t,
            0,
            "check(pool user)",
            "-",
            "ok",
            &format!("{check_pool:?}"),
            Some(check_pool.is_ok()),
        );
        row(
            t,
            0,
            "check(admin)",
            "-",
            "refused",
            &format!("{check_admin:?}"),
            Some(check_admin.is_err()),
        );
        pool_rows(t, w, exe);
        other_rows(t, w, exe);
        tally
    }

    /// The pool user's tokens, outside the range and inside it.
    fn pool_rows(t: &mut [usize; 3], w: &World, exe: &Path) {
        let rows: [(u32, &str, Derive); 7] = [
            (1, "pool-plain", Derive::Plain),
            (2, "pool-deny-only-user", Derive::DenyUser),
            (3, "pool-deny-only-user-and-group", Derive::DenyUserGroup),
            (4, "pool-b-token", Derive::BToken),
            (5, "pool-restricted-no-everyone", Derive::NoEveryone),
            (6, "pool-restricted-user-and-users", Derive::UserAndUsers),
            (13, "pool-low-integrity", Derive::Low),
        ];
        for (n, who, how) in rows {
            let token = tokens::derive(&w.pool, how, &w.user_sid, Some(&w.group_sid));
            let inside = match how {
                Derive::DenyUser | Derive::DenyUserGroup => "blocked",
                Derive::UserAndUsers => "measured",
                Derive::Plain | Derive::BToken | Derive::NoEveryone | Derive::Low => "reached",
            };
            for (target, port, expect) in [("outside", w.o, "blocked"), ("inside", w.i, inside)] {
                let (got, pass) = probe(&token, exe, &format!("connect:127.0.0.1:{port}"), expect);
                row(t, n, who, target, expect, &got, pass);
            }
        }
        let lowbox = format!("derived:lowbox:connect:127.0.0.1:{}", w.o);
        let token = tokens::derive(&w.pool, Derive::Plain, &w.user_sid, None);
        let (got, pass) = probe(&token, exe, &lowbox, "blocked");
        row(
            t,
            11,
            "pool-appcontainer-child",
            "outside",
            "blocked",
            &got,
            pass,
        );
    }

    /// Tokens that aren't the pool's: none should be caught but by measured collateral.
    fn other_rows(t: &mut [usize; 3], w: &World, exe: &Path) {
        let outside = format!("connect:127.0.0.1:{}", w.o);
        let rows: [(u32, &str, Derive, &str); 3] = [
            (7, "admin-own", Derive::Plain, "reached"),
            (
                8,
                "admin-restricted-admin-and-users",
                Derive::UserAndUsers,
                "measured",
            ),
            (
                9,
                "admin-restricted-with-restricted",
                Derive::NoEveryone,
                "reached",
            ),
        ];
        for (n, who, how, expect) in rows {
            let token = tokens::derive(&w.admin, how, &w.admin_sid, None);
            let (got, pass) = probe(&token, exe, &outside, expect);
            row(t, n, who, "outside", expect, &got, pass);
        }
        let lowbox = format!("derived:lowbox:{outside}");
        let token = tokens::derive(&w.admin, Derive::Plain, &w.admin_sid, None);
        let (got, pass) = probe(&token, exe, &lowbox, "reached");
        row(
            t,
            12,
            "admin-appcontainer-child",
            "outside",
            "reached",
            &got,
            pass,
        );
        let (got, pass) = probe(&tokens::system(), exe, &outside, "reached");
        row(t, 10, "system", "outside", "reached", &got, pass);
    }

    /// Runs `step` under `token`; what happened, and whether it matches `expect`.
    fn probe(
        token: &Result<Token, String>,
        exe: &Path,
        step_text: &str,
        expect: &str,
    ) -> (String, Option<bool>) {
        let token = match token {
            Ok(token) => token,
            Err(why) => return (format!("skip:{why}"), None),
        };
        match tokens::run_as(token, exe, step_text) {
            Ok(code) => match step(code, 0) {
                Some(got) => {
                    let reached = got == Got::Reached;
                    let pass = match expect {
                        "reached" => Some(reached),
                        "blocked" => Some(!reached),
                        _ => None,
                    };
                    (format!("{got:?}").to_lowercase(), pass)
                }
                None => (format!("nostart:{code:#x}"), None),
            },
            Err(why) => (format!("nostart:{why}"), None),
        }
    }
}

#[cfg(windows)]
#[path = "wfp_denyonly_spike/sids.rs"]
mod sids;
