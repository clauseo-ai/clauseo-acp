//! The search thread's searches: one run with its generation checks, the query hold (a test
//! seam), and the searches answered between two pages of a walk.

use clauseo_protocol::{SearchAnswer, SearchRequest};

use super::Worker;
use crate::error::Error;
use crate::search::{Control, SearchReply, answer};

/// Runs one search, unless a newer one from its window came first.
pub(super) fn answer(
    worker: &mut Worker,
    window: &str,
    request: &SearchRequest,
    reply: SearchReply,
) {
    let generation = request.generation.get();
    if !worker.shared.generations().start(window, generation) {
        reply(Ok(SearchAnswer::Cancelled));
        return;
    }
    hold_query(worker);
    let query = answer::query(request, worker.clock.now().wall);
    let result = worker.index.query(&query);
    let superseded = {
        let mut generations = worker.shared.generations();
        generations.finish();
        generations.superseded(window, generation)
    };
    let answered = match result {
        // A late answer to an older search is never delivered.
        Ok(_) | Err(clauseo_search::Error::Interrupted) if superseded => {
            Ok(SearchAnswer::Cancelled)
        }
        Err(clauseo_search::Error::Interrupted) => Ok(SearchAnswer::Cancelled),
        Ok(page) => Ok(SearchAnswer::Page(answer::page(page))),
        Err(error) => Err(Error::Internal(format!("search: {error}"))),
    };
    reply(answered);
}

/// Stops before a query while the query hold is on (test seam). Jobs that come meanwhile
/// wait their turn; a host that ends lets the thread go.
#[cfg(feature = "test-support")]
fn hold_query(worker: &mut Worker) {
    use std::pin::pin;

    use futures::executor::block_on;
    use futures::future::{Either, select};

    let Some(hold) = worker.seams.query_hold.clone() else {
        return;
    };
    if hold.is_released() {
        return;
    }
    hold.note_reached();
    loop {
        let woke = block_on(async {
            let released = pin!(hold.wait());
            let job = pin!(worker.control.recv());
            match select(released, job).await {
                Either::Right((Ok(job), _)) => Some(job),
                Either::Left(((), _)) | Either::Right((Err(_), _)) => None,
            }
        });
        match woke {
            Some(job) => worker.deferred.push_back(job),
            None => return,
        }
    }
}

/// Without the test seams nothing holds a query.
#[cfg(not(feature = "test-support"))]
const fn hold_query(_worker: &mut Worker) {}

/// Answers the searches queued now, keeping the other control jobs for later: a long walk
/// does not hold searches up.
pub(super) fn answer_queued(worker: &mut Worker) {
    while let Ok(job) = worker.control.try_recv() {
        if let Control::Query {
            window,
            request,
            reply,
        } = job
        {
            answer(worker, &window, &request, reply);
        } else {
            worker.deferred.push_back(job);
        }
    }
}
