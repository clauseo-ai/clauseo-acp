//! Rows 3 and 5 of the reclaim VM run, split from `rows.rs` for its size.

use std::path::{Path, PathBuf};

use super::{
    ApplyFailure, Ctx, MATTERS, Matter, Out, Owned, Sid, Stuck, allows, owned_by, profile_of,
    result_line, sddl, shown, win,
};

/// Row 3, once rows 1 and 2 are reclaimed, with a reclaim of its own: matter three's launch
/// meets nothing of matter one's, so its first refusal is its own `doc.txt`.
pub(super) fn row3(out: &mut Out, ctx: &Ctx, y: &Sid) -> Result<(), String> {
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
pub(super) fn row5(out: &mut Out, ctx: &Ctx, exe: &Path, three: &Matter) -> Result<(), String> {
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
