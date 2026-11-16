//! `prepare` (a normal, Medium PowerShell: the lawyer's own token, as the app runs): the data
//! folder, two matters with files, set-up through the exe as the app runs it, and each matter
//! launched in the launcher's order, so they carry the pool entries, Low labels and fences a
//! launch writes. Matter two's `assign` may be refused until main's pool-reuse fix; it is then
//! applied as an unused pool user without `assign` (`Ctx::launch`), so it is not registered.

use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clauseo_protocol::MatterId;
use clauseo_sandbox::windows::acl_plan::{Subjects, plan};
use clauseo_sandbox::windows::admit::{ReadDir, admit};
use clauseo_sandbox::windows::apply::{apply, repair_after_session};
use clauseo_sandbox::windows::ledger::{Ledger, ledger_path};
use clauseo_sandbox::windows::matters::MatterSids;
use clauseo_sandbox::windows::setup::around::Around;
use clauseo_sandbox::windows::setup::registered::registered;
use clauseo_sandbox::windows::sids::{Reserved, Sid};
use clauseo_sandbox::windows::{
    Neighbours, NtfsAcl, after_session, before_launch, sandbox_group_sid, sandbox_owner_sid,
    sandbox_pool_sids,
};
use clauseo_sandbox::{Policy, SandboxSetup, UserSetup};

use crate::{Out, ROOT, win};

/// One matter: its folder name under `matters\` and `state\`, and its id.
struct Matter {
    name: &'static str,
    id: &'static str,
}

const MATTERS: [Matter; 2] = [
    Matter {
        name: "one",
        id: "01890a5d-ac96-774b-bcce-b302099a8201",
    },
    Matter {
        name: "two",
        id: "01890a5d-ac96-774b-bcce-b302099a8202",
    },
];

/// The files each matter holds, relative to its folder: a few at the top, a nested folder.
const FILES: [(&str, &[u8]); 4] = [
    ("brief.txt", b"The client's instructions.\r\n"),
    ("notes.md", b"# Notes\n\nNothing here may change.\n"),
    (r"drafts\v1.txt", b"First draft.\r\n"),
    (r"drafts\exhibits\a.bin", &[0, 1, 2, 3, 254, 255]),
];

impl Matter {
    fn id(&self) -> Result<MatterId, String> {
        self.id
            .parse()
            .map_err(|_bad| format!("bad matter id {}", self.id))
    }

    fn root(&self) -> PathBuf {
        Path::new(ROOT).join("matters").join(self.name)
    }

    fn state(&self) -> PathBuf {
        Path::new(ROOT).join("state").join(self.name)
    }
}

/// `Modify`, `WRITE_DAC` and `WRITE_OWNER`: the lawyer's explicit entry on each matter folder.
const LAWYER_ON_MATTER: &str = "0x1f01bf";

/// The data folder: the lawyer's, made at Medium.
fn data_dir() -> PathBuf {
    Path::new(ROOT).join("data")
}

/// What the launches share: the data folder and the pool; and a clock of the harness's own, so
/// each launch's set-aside pass has its own time and session id.
struct Ctx {
    data: PathBuf,
    pool: Vec<Sid>,
    group: Sid,
    ticks: Cell<u64>,
}

/// A launch done: the pool user it ran as, whether `assign` gave it, and what `apply` did.
struct Launched {
    user: Sid,
    how: String,
}

impl Ctx {
    fn ledger(&self) -> Result<Ledger, String> {
        let mut trusted = self.pool.clone();
        trusted.push(self.group.clone());
        let reserved = Reserved::of_this_machine().map_err(|e| e.to_string())?;
        Ledger::open(ledger_path(&self.data), &trusted, &reserved).map_err(|e| e.to_string())
    }

    /// The next tick of the harness's own clock (the crate takes time as input; no wall clock
    /// is read).
    fn now(&self) -> SystemTime {
        let tick = self.ticks.get() + 1;
        self.ticks.set(tick);
        UNIX_EPOCH + Duration::from_secs(1_800_000_000 + tick)
    }

    /// Gives `m` a pool user, as the launcher will (`MatterSids::assign`): this registers it.
    fn assign(&self, m: &Matter, session: &str) -> Result<Sid, String> {
        let mut matters = MatterSids::load(&self.data).map_err(|e| e.to_string())?;
        let open = matters.open_sessions().map_err(|e| e.to_string())?;
        let ledger = || self.ledger().map_err(io::Error::other);
        matters
            .assign(&m.id()?, session, &self.pool, ledger, &open)
            .map_err(|e| e.to_string())
    }

    /// `m`'s launch in the launcher's order: `before_launch` (which marks the session open),
    /// `assign`, the gate, the plan and `apply`; then, as if the agent exited at once,
    /// `repair_after_session` (when the launch passed) and `after_session`, which ends the
    /// session whatever happened. Ported from `PR4i`'s harness (`2a3caacf8`).
    ///
    /// `fallback`: when `assign` refuses (until main's fix, any ledger entry holds a pool user
    /// back, so a second matter's assign finds them all held back), the launch runs as that
    /// pool user instead, unassigned: the same gate, plan and `apply`, so the matter carries the
    /// same entries, but it isn't registered in `matters.json`.
    fn launch(&self, m: &Matter, fallback: Option<&Sid>) -> Result<Launched, String> {
        let id = m.id()?;
        let policy = Policy::builder(m.root(), m.state())
            .build()
            .map_err(|e| e.to_string())?;
        let session = format!("vmx-{}-{}", m.name, self.ticks.get());
        before_launch(&policy, &self.data, &id, &session, &[], self.now())
            .map_err(|e| format!("before_launch: {e}"))?;
        let picked = match (self.assign(m, &session), fallback) {
            (Ok(user), _) => Ok((user, "assigned".to_owned())),
            (Err(refused), Some(user)) => Ok((
                user.clone(),
                format!("assign refused ({refused}), so applied as {user} unassigned"),
            )),
            (Err(refused), None) => Err(format!("assign: {refused}")),
        };
        let result = picked.and_then(|(user, how)| {
            let applied = self.apply(&policy, id, &user)?;
            Ok(Launched {
                user,
                how: format!("{how}; {applied}"),
            })
        });
        let mut ended = Vec::new();
        if result.is_ok() {
            let repaired = self.ledger().and_then(|ledger| {
                let mut acl = NtfsAcl::new().map_err(|e| e.to_string())?;
                let mut around = Around::new(self.data.clone());
                repair_after_session(&ledger, id, &mut acl, &mut around).map_err(|e| e.to_string())
            });
            if let Err(error) = repaired {
                ended.push(format!("repair_after_session: {error}"));
            }
        }
        if let Err(error) = after_session(&policy, &self.data, &id, &session, self.now()) {
            ended.push(format!("after_session: {error}"));
        }
        match result {
            Ok(done) if ended.is_empty() => Ok(done),
            Ok(done) => Err(format!("{}; but {}", done.how, ended.join("; "))),
            Err(refused) if ended.is_empty() => Err(refused),
            Err(refused) => Err(format!("{refused}; and {}", ended.join("; "))),
        }
    }

    /// The gate with the registered matters, the plan for `pool_user`, then `apply` through
    /// the lawyer's `NtfsAcl`.
    fn apply(&self, policy: &Policy, id: MatterId, pool_user: &Sid) -> Result<String, String> {
        let mut ledger = self.ledger()?;
        let matters = MatterSids::load(&self.data).map_err(|e| e.to_string())?;
        let others = registered(matters.rows(), &ledger).into_iter();
        let neighbours = Neighbours {
            registered: others.filter(|r| r.id != id).collect(),
            reach: Vec::new(),
        };
        let admitted =
            admit(policy, &self.data, &neighbours, &ReadDir).map_err(|e| e.to_string())?;
        let subjects = Subjects {
            other_pool_users: self
                .pool
                .iter()
                .filter(|p| *p != pool_user)
                .cloned()
                .collect(),
            pool_user: pool_user.clone(),
            matter: None,
            pool_group: self.group.clone(),
        };
        let mut plan = plan(&admitted, &subjects, id).map_err(|e| e.to_string())?;
        let mut acl = NtfsAcl::new().map_err(|e| e.to_string())?;
        let mut around = Around::new(self.data.clone());
        let applied =
            apply(&mut ledger, &mut plan, &mut acl, &mut around).map_err(|e| e.to_string())?;
        Ok(format!(
            "written {}, walked {:?}",
            applied.written, applied.walked
        ))
    }
}

/// Makes the folders and files under `C:\ClauseoVmX`, all as the lawyer.
fn make_tree(me: &str) -> Result<(), String> {
    for dir in [
        PathBuf::from(ROOT),
        data_dir(),
        Path::new(ROOT).join("matters"),
        Path::new(ROOT).join("state"),
    ] {
        win::make_dir(&dir)?;
    }
    for m in &MATTERS {
        win::make_dir(&m.root())?;
        // The lawyer owns it (made at Medium) and gets an explicit entry with WRITE_OWNER, which
        // the Low label's write needs (`PR4i`'s `372bbd94d`), merged with what is inherited.
        win::set_dacl(
            &m.root(),
            &format!("D:(A;OICI;{LAWYER_ON_MATTER};;;{me})"),
            false,
        )?;
        win::make_dir(&m.state())?;
        win::make_dir(&m.root().join("drafts"))?;
        win::make_dir(&m.root().join(r"drafts\exhibits"))?;
        for (name, bytes) in FILES {
            win::make_file(&m.root().join(name), bytes)?;
        }
    }
    Ok(())
}

/// The `prepare` step.
pub(crate) fn prepare(out: &mut Out, exe: &Path) -> Result<(), String> {
    if win::elevated()? {
        return Err("run this step from a normal (Medium) PowerShell, not an elevated one".into());
    }
    if sandbox_owner_sid().map_err(|e| e.to_string())?.is_some() {
        return Err("this machine is set up already: use a fresh VM (or `cleanup` first)".into());
    }
    if Path::new(ROOT).exists() {
        return Err(format!(
            "{ROOT} is left over: run `cleanup` (elevated) first"
        ));
    }
    if !exe.exists() {
        return Err(format!("no setup exe at {} (pass --exe)", exe.display()));
    }
    let me = win::current_user_sid()?;
    make_tree(&me)?;
    for m in &MATTERS {
        let sddl = win::sddl_of(&m.root())?;
        out.evidence(&format!("{} as made", m.root().display()), &sddl);
        out.check(
            sddl.starts_with(&format!("O:{me}D:"))
                || (me.ends_with("-500") && sddl.starts_with("O:LAD:")),
            &format!("the lawyer owns matter {}", m.name),
        );
    }
    let data = data_dir();
    // `UserSetup` starts only the admin copy in %ProgramFiles%\Clauseo\Sandbox\, never `exe`:
    // place it there first and run it once with `--adopt` from an elevated prompt.
    let setup = UserSetup::new(data.clone());
    out.line("set-up: UserSetup::run() (the exe raises its prompt)");
    setup.run().map_err(|e| format!("set-up failed: {e}"))?;
    out.evidence("state after set-up", &format!("{:?}", setup.state()));
    let pool = sandbox_pool_sids().map_err(|e| e.to_string())?;
    let group = sandbox_group_sid()
        .map_err(|e| e.to_string())?
        .ok_or("no GroupSid in the record")?;
    let ctx = Ctx {
        data,
        pool,
        group: Sid::parse(&group).map_err(|e| e.to_string())?,
        ticks: Cell::new(0),
    };
    out.evidence("pool", &format!("{} users", ctx.pool.len()));
    // Matter one first; matter two may fall back to an unassigned pool user (`Ctx::launch`).
    let mut users: Vec<Sid> = Vec::new();
    for m in &MATTERS {
        let unused = ctx.pool.iter().find(|p| !users.contains(p));
        let fallback = if users.is_empty() { None } else { unused };
        let launched = ctx.launch(m, fallback)?;
        out.evidence(
            &format!("launch {}", m.name),
            &format!("pool user {}: {}", launched.user, launched.how),
        );
        users.push(launched.user);
    }
    for (m, user) in MATTERS.iter().zip(&users) {
        let sddl = win::sddl_of(&m.root())?;
        out.evidence(&m.root().display().to_string(), &sddl);
        out.check(
            sddl.contains(user.as_str()) && sddl.contains("(ML;OICI;NW;;;LW)"),
            &format!(
                "matter {} holds its pool user's entry and the Low label",
                m.name
            ),
        );
    }
    let device_sync = device_sync();
    let shared = win::sddl_of(&device_sync).unwrap_or_else(|e| format!("unreadable ({e})"));
    out.evidence(&device_sync.display().to_string(), &shared);
    out.check(shared.contains(&group), "DeviceSync holds the group's deny");
    Ok(())
}

/// `C:\ProgramData\Microsoft\DeviceSync`, every pool user's shared Low-writable folder.
pub(crate) fn device_sync() -> PathBuf {
    let program_data = std::env::var_os("ProgramData").unwrap_or_else(|| r"C:\ProgramData".into());
    PathBuf::from(program_data)
        .join("Microsoft")
        .join("DeviceSync")
}
