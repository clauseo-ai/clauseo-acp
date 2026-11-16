//! The runs after the redline's view that check it, off the actor: `stetdoc check --json` on the
//! redline, whose list of changes by id the blackline's body is checked against
//! (`parse/ledger.rs`), and, when the two versions differ, `stetdoc view --all --json --mode
//! final` on each, whose links, comments and text boxes are compared (`parse/sides.rs`): stetdoc
//! 0.9.3's compare marks no change for a link's new address or a comment's new text.
//!
//! It answers what a run that fails means: never a failed blackline. A check that fails leaves
//! the count from the view's summary; a version's view that fails means the links and comments
//! could not be compared, so the blackline may differ. The log gets the step and the rule, never
//! a document's text.
//!
//! The pin: launch ships stetdoc 0.9.3 (`crates/desktop/stetdoc.toml`). Its next release (its
//! PR #22) mends the compare of an inner table that ends its cell and adds words to tags the
//! reader already takes (`{… label}`, `{row … item=2}`, `parse/tests/forms.rs`). When it is
//! pinned: the fixtures are made again, `blackline.nested_table` stays for safety, and the
//! comparison of links and comments stays as a check until a release whose compare marks them is
//! pinned and seen to mark them.

use std::ffi::OsString;
use std::path::Path;

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::{BlacklineNotice, WordBlackline};

use super::{REDLINE, VIEW_LIMIT, author_arg, run};
use crate::actor::query::blackline::parse::ledger::{self, Change};
use crate::actor::query::blackline::parse::sides::{Crossed, Side, cross, read_side};
use crate::actor::query::blackline::parse::{Read, order};
use crate::config::Stetdoc;
use crate::matter_worker::{NEW, OLD};

/// The most changed links the blackline names; past it, it says the rest may differ.
pub(crate) const LINKS_MAX: usize = 50;

/// stetdoc's list of the changes of the redline in `dir` (`stetdoc check --json`), or why there
/// is none.
pub(crate) async fn list_changes<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Stetdoc,
    dir: &Path,
) -> Result<Vec<Change>, String> {
    let check = vec![
        "check".into(),
        "--json".into(),
        author_arg(&stetdoc.author),
        dir.join(REDLINE).into_os_string(),
    ];
    run(launcher, timer, stetdoc, dir, "check", check, VIEW_LIMIT)
        .await
        .map_err(|failed| failed.to_string())
        .and_then(|out| ledger::read_check(&out).map_err(str::to_owned))
}

/// Checks `read`, the redline's view in `dir` with its parts: against stetdoc's list of the
/// redline's changes (`listed`, [`list_changes`]), and, when the versions differ (`differ`),
/// against what each version's view shows.
pub(crate) async fn cross_check<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Stetdoc,
    dir: &Path,
    read: &mut Read,
    listed: Result<Vec<Change>, String>,
    differ: bool,
) {
    let path = |name: &str| dir.join(name).into_os_string();
    match listed {
        Ok(changes) => ledger::apply(read, &changes),
        Err(why) => tracing::info!(%why, "the Word blackline is checked by the view's summary"),
    }
    if !differ {
        return;
    }
    let old = side(launcher, timer, stetdoc, dir, path(OLD)).await;
    let new = match old {
        Ok(_) => side(launcher, timer, stetdoc, dir, path(NEW)).await,
        Err(_) => Err(String::new()),
    };
    match (old, new) {
        (Ok(old), Ok(new)) => note_what_compare_missed(&mut read.blackline, cross(&old, &new)),
        (Err(why), _) | (_, Err(why)) => {
            tracing::info!(%why, "the Word blackline's versions could not be compared");
            read.blackline.notices.push(BlacklineNotice::MayDiffer);
            order(&mut read.blackline.notices);
        }
    }
}

/// One version's view (`version`, its path), read.
async fn side<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Stetdoc,
    dir: &Path,
    version: OsString,
) -> Result<Side, String> {
    let view = vec![
        "view".into(),
        "--all".into(),
        "--json".into(),
        "--mode".into(),
        "final".into(),
        version,
    ];
    let out = run(
        launcher,
        timer,
        stetdoc,
        dir,
        "version view",
        view,
        VIEW_LIMIT,
    )
    .await
    .map_err(|failed| failed.to_string())?;
    read_side(&out).map_err(|why| format!("stetdoc version view: {why}"))
}

/// Says in `blackline` what the comparison of the versions' views found: the links whose
/// address changed, by their text (the first [`LINKS_MAX`]); a comment's or a text box's text
/// that changed; and that it may differ for a link with no text, or links past the cap.
pub(crate) fn note_what_compare_missed(blackline: &mut WordBlackline, crossed: Crossed) {
    let Crossed {
        mut links,
        unnamed_link,
        comments,
        boxes,
    } = crossed;
    let notices = &mut blackline.notices;
    if unnamed_link || links.len() > LINKS_MAX {
        notices.push(BlacklineNotice::MayDiffer);
    }
    links.truncate(LINKS_MAX);
    blackline.changed_links = links;
    if comments {
        notices.push(BlacklineNotice::CommentChanged);
    }
    if boxes {
        notices.push(BlacklineNotice::TextBoxChanged);
    }
    order(notices);
}
