//! The runs that show the redline's footnotes, endnotes, headers and footers, off the actor:
//! `stetdoc view --all --json --story notes` and `--story headers` on the redline, read by
//! `parse/story.rs` into the blackline's parts.
//!
//! It answers what a run that fails means: never a failed blackline. The parts of that story
//! are not shown, and their changes' ids are not counted as shown, so stetdoc's list of changes
//! (`parse/ledger.rs`), or its summary's count when there is no list, says there are changes
//! outside what the blackline shows. The log gets the step and the rule, never a document's
//! text.

use std::path::Path;

use clauseo_process::{Launcher, Timer};
use futures::future::join;

use super::{REDLINE, VIEW_LIMIT, run};
use crate::actor::query::blackline::parse::story::{Story, StoryRead, read_story};
use crate::config::Stetdoc;

/// The redline's notes and headers in `dir`, read (both story views, run at once), for
/// `add_parts`, and whether the headers' view read; a story whose view fails is left out.
pub(crate) async fn view_stories<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Stetdoc,
    dir: &Path,
) -> (Vec<StoryRead>, bool) {
    let (notes, headers) = join(
        view_story(launcher, timer, stetdoc, dir, Story::Notes),
        view_story(launcher, timer, stetdoc, dir, Story::Headers),
    )
    .await;
    let headers_read = headers.is_ok();
    let mut stories = Vec::new();
    for (story, viewed) in [(Story::Notes, notes), (Story::Headers, headers)] {
        match viewed {
            Ok(viewed) => stories.push(viewed),
            Err(why) => {
                tracing::info!(%why, story = story.word(), "a story of the Word blackline is not shown");
            }
        }
    }
    (stories, headers_read)
}

/// The redline's view of `story`, read.
async fn view_story<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Stetdoc,
    dir: &Path,
    story: Story,
) -> Result<StoryRead, String> {
    let view = vec![
        "view".into(),
        "--all".into(),
        "--json".into(),
        "--story".into(),
        story.word().into(),
        dir.join(REDLINE).into_os_string(),
    ];
    let out = run(
        launcher,
        timer,
        stetdoc,
        dir,
        "story view",
        view,
        VIEW_LIMIT,
    )
    .await
    .map_err(|failed| failed.to_string())?;
    read_story(&out, story).map_err(|why| format!("stetdoc story view: {why}"))
}
