//! `run` (a normal, Medium PowerShell: the lawyer's own token): set-up through the exe, four
//! matters launched in the launcher's order (`Ctx::launch`), each row's state made as a pool
//! user, `UserSetup::reclaim()` (the exe, then the lawyer's hand-back), the forget and the
//! launches retried, and the verdicts for rows 1, 2, 3 and 5.

use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clauseo_protocol::MatterId;
use clauseo_sandbox::windows::acl_plan::{Subjects, plan};
use clauseo_sandbox::windows::admit::{ReadDir, admit};
use clauseo_sandbox::windows::apply::{apply, repair_after_session};
use clauseo_sandbox::windows::ledger::{Ledger, Stuck, ledger_path};
use clauseo_sandbox::windows::matters::MatterSids;
use clauseo_sandbox::windows::secrets::secret_for;
use clauseo_sandbox::windows::setup::around::Around;
use clauseo_sandbox::windows::setup::profile::profile_of;
use clauseo_sandbox::windows::setup::registered::registered;
use clauseo_sandbox::windows::setup::result_file;
use clauseo_sandbox::windows::sids::{Reserved, Sid};
use clauseo_sandbox::windows::{
    ApplyFailure, Neighbours, NtfsAcl, Owned, after_session, before_launch, sandbox_group_sid,
    sandbox_pool_sids,
};
use clauseo_sandbox::{Error, Policy, SandboxSetup, UserSetup};

use crate::{Out, ROOT, win};

/// One matter of the run: its folder name under `m\` and its id.
pub(crate) struct Matter {
    pub(crate) name: &'static str,
    id: &'static str,
}

pub(crate) const MATTERS: [Matter; 4] = [
    Matter {
        name: "one",
        id: "01890a5d-ac96-774b-bcce-b302099a8101",
    },
    Matter {
        name: "two",
        id: "01890a5d-ac96-774b-bcce-b302099a8102",
    },
    Matter {
        name: "three",
        id: "01890a5d-ac96-774b-bcce-b302099a8103",
    },
    Matter {
        name: "four",
        id: "01890a5d-ac96-774b-bcce-b302099a8104",
    },
];

impl Matter {
    fn id(&self) -> Result<MatterId, String> {
        self.id
            .parse()
            .map_err(|_bad| format!("bad matter id {}", self.id))
    }

    pub(crate) fn root(&self) -> PathBuf {
        Path::new(ROOT).join("m").join(self.name)
    }

    fn state(&self) -> PathBuf {
        Path::new(ROOT).join("state").join(self.name)
    }
}

/// The data folder: the lawyer's, made at Medium.
pub(crate) fn data_dir() -> PathBuf {
    Path::new(ROOT).join("data")
}

/// What the rows share: the set-up, the lawyer, the pool; and a clock and a session counter of
/// their own, so each launch's set-aside pass has its own time and session id.
struct Ctx {
    setup: UserSetup,
    data: PathBuf,
    me: String,
    pool: Vec<Sid>,
    group: Sid,
    ticks: Cell<u64>,
}

/// A launch refused: the failure and the item, when the refusal was an ACL apply's.
struct Refusal {
    failure: Option<ApplyFailure>,
    at: Option<PathBuf>,
    text: String,
}

impl Refusal {
    fn other(text: impl Into<String>) -> Refusal {
        Refusal {
            failure: None,
            at: None,
            text: text.into(),
        }
    }

    fn of(error: &Error) -> Refusal {
        match error {
            Error::AclApply { failure, at, .. } => Refusal {
                failure: Some(*failure),
                at: at.clone(),
                text: error.to_string(),
            },
            _ => Refusal::other(error.to_string()),
        }
    }
}

impl Ctx {
    fn ledger(&self) -> Result<Ledger, String> {
        let mut trusted = self.pool.clone();
        trusted.push(self.group.clone());
        let reserved = Reserved::of_this_machine().map_err(|e| e.to_string())?;
        Ledger::open(ledger_path(&self.data), &trusted, &reserved).map_err(|e| e.to_string())
    }

    /// Gives `m` a pool user, as the launcher will (`MatterSids::assign`).
    fn assign(&self, m: &Matter, session: &str) -> Result<Sid, String> {
        let mut matters = MatterSids::load(&self.data).map_err(|e| e.to_string())?;
        let open = matters.open_sessions().map_err(|e| e.to_string())?;
        let ledger = || self.ledger().map_err(io::Error::other);
        matters
            .assign(&m.id()?, session, &self.pool, ledger, &open)
            .map_err(|e| e.to_string())
    }

    /// The next tick of the harness's own clock: a time after the last, for the set-aside's
    /// pass folders (the crate takes time as input; no wall clock is read).
    fn now(&self) -> SystemTime {
        let tick = self.ticks.get() + 1;
        self.ticks.set(tick);
        UNIX_EPOCH + Duration::from_secs(1_800_000_000 + tick)
    }

    /// The pool user `m` holds.
    fn pool_user(&self, m: &Matter) -> Result<Sid, String> {
        let matters = MatterSids::load(&self.data).map_err(|e| e.to_string())?;
        let user = matters.pool_user(&m.id()?).cloned();
        user.ok_or_else(|| format!("matter {} holds no pool user", m.name))
    }

    /// `m`'s launch in the order the launcher will run it: `before_launch` (the set-aside's
    /// check, which marks the session open), `assign` (which refuses a new pick for a matter
    /// with no open session), the gate, the plan and `apply`; then, as if the agent exited at
    /// once, `repair_after_session` (when the launch passed) and `after_session`, which takes
    /// the session out of the open ones whatever happened, as the launcher does when a launch
    /// it started ends.
    fn launch(&self, m: &Matter) -> Result<String, Refusal> {
        let id = m.id().map_err(Refusal::other)?;
        let policy = Policy::builder(m.root(), m.state())
            .build()
            .map_err(|e| Refusal::of(&e))?;
        let session = format!("vmr-{}-{}", m.name, self.ticks.get());
        before_launch(&policy, &self.data, &id, &session, &[], self.now())
            .map_err(|e| Refusal::of(&e))?;
        let result = self
            .assign(m, &session)
            .map_err(Refusal::other)
            .and_then(|_| self.apply(&policy, id));
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
            Ok(done) => Err(Refusal::other(format!("{done}; but {}", ended.join("; ")))),
            Err(mut refused) => {
                if !ended.is_empty() {
                    refused.text = format!("{}; and {}", refused.text, ended.join("; "));
                }
                Err(refused)
            }
        }
    }

    /// The gate, the plan and `apply` of `m`'s launch, once its session is marked and its pool
    /// user assigned.
    fn apply(&self, policy: &Policy, id: MatterId) -> Result<String, Refusal> {
        let mut ledger = self.ledger().map_err(Refusal::other)?;
        let matters = MatterSids::load(&self.data).map_err(|e| Refusal::other(e.to_string()))?;
        let pool_user = matters
            .pool_user(&id)
            .cloned()
            .ok_or_else(|| Refusal::other("the matter has no pool user"))?;
        let others = registered(matters.rows(), &ledger).into_iter();
        let neighbours = Neighbours {
            registered: others.filter(|r| r.id != id).collect(),
            reach: Vec::new(),
        };
        let admitted =
            admit(policy, &self.data, &neighbours, &ReadDir).map_err(|e| Refusal::of(&e))?;
        let subjects = Subjects {
            other_pool_users: self
                .pool
                .iter()
                .filter(|p| **p != pool_user)
                .cloned()
                .collect(),
            pool_user,
            matter: None,
            pool_group: self.group.clone(),
        };
        let mut plan = plan(&admitted, &subjects, id).map_err(|e| Refusal::of(&e))?;
        let mut acl = NtfsAcl::new().map_err(|e| Refusal::other(e.to_string()))?;
        let mut around = Around::new(self.data.clone());
        let applied =
            apply(&mut ledger, &mut plan, &mut acl, &mut around).map_err(|e| Refusal::of(&e))?;
        Ok(format!(
            "written {}, walked {:?}",
            applied.written, applied.walked
        ))
    }

    /// Runs `f` as the pool user `sid`, logged on with its password from `secrets.bin` (read as
    /// the lawyer, as PR5's runner will).
    fn as_pool<T>(&self, sid: &Sid, f: impl FnOnce() -> T) -> Result<T, String> {
        let name = win::account_name(sid.as_str())?;
        let password = secret_for(&self.data, sid).map_err(|e| e.to_string())?;
        win::as_user(&name, &password, f)
    }

    /// The ledger's clean-ups owed and hand-backs owed, for an evidence line.
    fn owed(&self) -> String {
        let Ok(ledger) = self.ledger() else {
            return "the ledger can't be read".to_owned();
        };
        let name = |id: &MatterId| {
            let found = MATTERS.iter().find(|m| m.id().ok().as_ref() == Some(id));
            found.map_or("?", |m| m.name)
        };
        let reclaims: Vec<String> = ledger
            .reclaims_owed()
            .iter()
            .map(|o| format!("{}:{:?}:{}", name(&o.matter), o.why, o.path.display()))
            .collect();
        let handbacks: Vec<String> = ledger
            .handbacks_owed()
            .iter()
            .map(|o| name(&o.matter).to_owned())
            .collect();
        format!(
            "reclaims [{}]; handbacks owed [{}]",
            reclaims.join(", "),
            handbacks.join(", ")
        )
    }

    /// Whether the ledger lists a clean-up owed for `m`.
    fn reclaim_listed(&self, m: &Matter, why: Stuck) -> bool {
        let (Ok(ledger), Ok(id)) = (self.ledger(), m.id()) else {
            return false;
        };
        ledger
            .reclaims_owed()
            .iter()
            .any(|o| o.matter == id && o.why == why)
    }

    fn any_reclaim_for(&self, m: &Matter) -> bool {
        let (Ok(ledger), Ok(id)) = (self.ledger(), m.id()) else {
            return true;
        };
        ledger.reclaims_owed().iter().any(|o| o.matter == id)
    }
}

/// `path`'s owner and DACL as SDDL, or why not.
fn sddl(path: &Path) -> String {
    win::sddl_of(path).unwrap_or_else(|e| format!("unreadable ({e})"))
}

/// Whether the SDDL names `sid` as its owner (SDDL writes the built-in Administrator, RID 500,
/// as `LA`).
fn owned_by(sddl: &str, sid: &str) -> bool {
    sddl.strip_prefix("O:")
        .and_then(|rest| rest.split("D:").next())
        .is_some_and(|owner| owner == sid || (owner == "LA" && sid.ends_with("-500")))
}

/// Whether the SDDL holds an allow entry for `sid`.
fn allows(sddl: &str, sid: &str) -> bool {
    sddl.split('(')
        .any(|ace| ace.starts_with("A;") && ace.trim_end_matches(')').ends_with(sid))
}

/// `result` of a launch for an evidence line.
fn shown(result: &Result<String, Refusal>) -> String {
    match result {
        Ok(done) => format!("passed ({done})"),
        Err(refused) => format!("refused: {} (at {:?})", refused.text, refused.at),
    }
}

/// The result file the exe wrote last.
fn result_line(data: &Path) -> (String, Option<u64>) {
    match result_file::read(data) {
        Ok(Some(r)) => (
            format!(
                "exit {} step {:?} cause {:?} taken {:?}",
                r.exit, r.step, r.cause, r.taken
            ),
            r.taken,
        ),
        Ok(None) => ("no result file".to_owned(), None),
        Err(e) => (format!("unreadable result file ({e})"), None),
    }
}

/// The `run` step.
pub(crate) fn run(out: &mut Out, exe: &Path) -> Result<(), String> {
    if win::elevated()? {
        return Err("run this step from a normal (Medium) PowerShell, not an elevated one".into());
    }
    if !MATTERS[0].root().exists() {
        return Err("run `prepare` first, from an elevated PowerShell".into());
    }
    if !exe.exists() {
        return Err(format!("no setup exe at {} (pass --exe)", exe.display()));
    }
    let data = data_dir();
    win::make_dir(&data)?;
    win::make_dir(&Path::new(ROOT).join("state"))?;
    for m in &MATTERS {
        win::make_dir(&m.state())?;
    }
    let setup = UserSetup::new(data.clone(), exe.to_owned()).map_err(|e| e.to_string())?;
    out.line("set-up: UserSetup::run() (the exe raises its prompt)");
    setup.run().map_err(|e| format!("set-up failed: {e}"))?;
    out.evidence("state after set-up", &format!("{:?}", setup.state()));
    let pool = sandbox_pool_sids().map_err(|e| e.to_string())?;
    let group = sandbox_group_sid()
        .map_err(|e| e.to_string())?
        .ok_or("no GroupSid in the record")?;
    let ctx = Ctx {
        setup,
        data,
        me: win::current_user_sid()?,
        pool,
        group: Sid::parse(&group).map_err(|e| e.to_string())?,
        ticks: Cell::new(0),
    };
    out.evidence("pool", &format!("{} users", ctx.pool.len()));
    let [one, _, three, four] = &MATTERS;
    for m in [one, three, four] {
        let first = ctx.launch(m);
        out.evidence(&format!("first launch {}", m.name), &shown(&first));
        if first.is_err() {
            return Err(format!("matter {}'s first launch failed", m.name));
        }
        out.evidence(
            &format!("matter {}", m.name),
            &format!("pool user {}", ctx.pool_user(m)?),
        );
    }
    let (a, y) = (ctx.pool_user(one)?, ctx.pool_user(four)?);
    rows(out, &ctx, exe, &a, &y)
}

/// Rows 1 and 2 (one reclaim for both), then row 3 (a reclaim of its own), then row 5.
fn rows(out: &mut Out, ctx: &Ctx, exe: &Path, a: &Sid, y: &Sid) -> Result<(), String> {
    rows12(out, ctx, a)?;
    row3(out, ctx, y)?;
    row5(out, ctx, exe, &MATTERS[2])
}

/// Rows 1 and 2, with one reclaim for both. Their state goes before row 3's is made: while
/// matter one is forgotten but owes its hand-back it is no registered matter but a folder beside
/// every other one, so each other matter's launch denies the pool group reading its tree and is
/// refused at its shut folder (row 2) before reaching anything of its own.
fn rows12(out: &mut Out, ctx: &Ctx, a: &Sid) -> Result<(), String> {
    let [one, two, ..] = &MATTERS;
    // Row 1: matter one's agent makes a folder only it can open, in a matter folder where the
    // lawyer has no FILE_DELETE_CHILD: forget can neither hand it back nor set it aside.
    let work = one.root().join("work");
    let shut = work.join("shut.txt");
    ctx.as_pool(a, || -> Result<(), String> {
        win::make_dir(&work)?;
        win::make_file(&shut, b"the agent's")?;
        win::set_dacl(&work, &format!("D:P(A;OICI;FA;;;{a})"), true)
    })??;
    let forgot_first = ctx.setup.forget_matter(&one.id()?);
    out.line("row 1 state made; row 2 next");
    // Row 2: matter two's first launch denies the pool group matter one's tree and meets the
    // shut folder.
    let two_first = ctx.launch(two);
    let owed_before = ctx.owed();
    let two_recorded = ctx.reclaim_listed(two, Stuck::ChildShut);
    out.line("rows 1 and 2, before reclaim:");
    out.evidence("forget one", &format!("{forgot_first:?}"));
    out.evidence("launch two", &shown(&two_first));
    out.evidence("ledger", &owed_before);
    for (label, path) in [("work", &work), ("shut.txt", &shut)] {
        out.evidence(label, &sddl(path));
    }
    out.line("UserSetup::reclaim() for rows 1 and 2 (the exe raises its prompt)");
    let reclaimed = ctx.setup.reclaim();
    out.evidence("reclaim", &format!("{reclaimed:?}"));
    out.evidence("result file", &result_line(&ctx.data).0);
    let forgot_again = ctx.setup.forget_matter(&one.id()?);
    let two_again = ctx.launch(two);
    out.line("rows 1 and 2, after reclaim:");
    out.evidence("forget one", &format!("{forgot_again:?}"));
    out.evidence("launch two", &shown(&two_again));
    out.evidence("ledger", &ctx.owed());
    let (work_after, shut_after) = (sddl(&work), sddl(&shut));
    for (label, text) in [("work", &work_after), ("shut.txt", &shut_after)] {
        out.evidence(label, text);
    }
    let me = ctx.me.as_str();
    let row1 = forgot_first.is_err()
        && owed_before.contains("handbacks owed [one")
        && reclaimed.is_ok()
        && forgot_again.is_ok()
        && owned_by(&work_after, me)
        && owned_by(&shut_after, me)
        && !work_after.contains(a.as_str())
        && !shut_after.contains(a.as_str());
    out.row(
        "1",
        row1,
        "C:\\m forget: owed, then reclaim and forget leave the lawyer owner, no ACE",
    );
    let shut_refused = two_first
        .as_ref()
        .err()
        .is_some_and(|r| r.failure == Some(ApplyFailure::ChildShut));
    let row2 = shut_refused && two_recorded && two_again.is_ok() && !ctx.any_reclaim_for(two);
    out.row(
        "2",
        row2,
        "another matter's launch refused ChildShut, passes after reclaim",
    );
    Ok(())
}

/// Row 3, once rows 1 and 2 are reclaimed, with a reclaim of its own: matter three's launch
/// meets nothing of matter one's, so its first refusal is its own `doc.txt`.
fn row3(out: &mut Out, ctx: &Ctx, y: &Sid) -> Result<(), String> {
    let [_, _, three, four] = &MATTERS;
    // Matter four's agent makes a file that denies the lawyer WRITE_OWNER; the lawyer moves it
    // into matter three, by the DELETE its inherited entry from matter four gives.
    let in_four = four.root().join("doc.txt");
    let doc = three.root().join("doc.txt");
    ctx.as_pool(y, || -> Result<(), String> {
        win::make_file(&in_four, b"another matter's")?;
        let dacl = format!("D:AI(D;;WO;;;{})(A;;FA;;;{y})", ctx.me);
        win::set_dacl(&in_four, &dacl, false)
    })??;
    win::move_file(&in_four, &doc)?;
    out.line("row 3 state made");
    let three_first = ctx.launch(three);
    let three_recorded = ctx.reclaim_listed(three, Stuck::PoolOwnedChild);
    out.line("row 3, before reclaim:");
    out.evidence("launch three", &shown(&three_first));
    out.evidence("ledger", &ctx.owed());
    out.evidence("doc.txt", &sddl(&doc));
    out.line("UserSetup::reclaim() for row 3 (the exe raises its prompt)");
    let reclaimed = ctx.setup.reclaim();
    out.evidence("reclaim", &format!("{reclaimed:?}"));
    out.evidence("result file", &result_line(&ctx.data).0);
    let doc_taken = sddl(&doc);
    let three_again = ctx.launch(three);
    out.line("row 3, after reclaim:");
    out.evidence("doc.txt after the hand-back", &doc_taken);
    out.evidence("launch three", &shown(&three_again));
    out.evidence("ledger", &ctx.owed());
    let doc_after = sddl(&doc);
    out.evidence("doc.txt", &doc_after);
    let child = ApplyFailure::PoolOwned {
        owned: Owned::Child,
    };
    let pool_owned = three_first.as_ref().err().is_some_and(|r| {
        r.failure == Some(child) && r.at.as_ref().is_some_and(|at| at.ends_with("doc.txt"))
    });
    let row3 = pool_owned
        && three_recorded
        && reclaimed.is_ok()
        && three_again.is_ok()
        && owned_by(&doc_after, ctx.me.as_str())
        && !allows(&doc_after, y.as_str())
        && !ctx.any_reclaim_for(three);
    out.row(
        "3",
        row3,
        "PoolOwned{Child} refuses, then launches after reclaim",
    );
    Ok(())
}

/// Row 5, the lawyer's half: clean-ups planted in the ledger at `C:\` and at a pool user's
/// profile are refused by the exe (exit 0, nothing taken); a later launch of the matter clears
/// them. The elevated `row4` step checks the profiles' owners. It needs a ledger owing nothing
/// first: the exe is run alone here, with no hand-back after, so anything a failed row left
/// recorded would be taken and left unstripped.
fn row5(out: &mut Out, ctx: &Ctx, exe: &Path, three: &Matter) -> Result<(), String> {
    let profile = ctx
        .pool
        .iter()
        .find_map(|sid| profile_of(sid).ok().flatten())
        .map(|p| p.path)
        .ok_or("no pool profile found")?;
    let mut ledger = ctx.ledger()?;
    let before = ledger.reclaims_owed().len();
    out.evidence("owed before planting", &ctx.owed());
    let id = three.id()?;
    for path in [PathBuf::from(r"C:\"), profile.clone()] {
        ledger
            .owe_reclaim(id, path, Stuck::ChildShut)
            .map_err(|e| e.to_string())?;
    }
    out.evidence("planted", &ctx.owed());
    let params = format!("--reclaim --parent {}", std::process::id());
    let code = win::run_exe(exe, &params)?;
    let (line, taken) = result_line(&ctx.data);
    let listed = ctx.ledger()?.reclaims_owed().len();
    out.evidence("exe --reclaim", &format!("exit {code}; {line}"));
    out.evidence("profile", &profile.display().to_string());
    let cleared = ctx.launch(three);
    out.evidence("launch three", &shown(&cleared));
    out.evidence("ledger", &ctx.owed());
    let row5 = before == 0
        && code == 0
        && taken == Some(0)
        && listed == 2
        && cleared.is_ok()
        && !ctx.any_reclaim_for(three);
    out.row(
        "5a",
        row5,
        "records at C:\\ and a pool profile are refused; nothing taken",
    );
    Ok(())
}
