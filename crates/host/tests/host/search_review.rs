//! REVIEW REPRO: a thread created just before its matter is forgotten, read in a lag's catch-up.

use std::thread;

use clauseo_host::SearchHold;
use clauseo_protocol::{
    Count, HostCommand, Principal, SearchAnswer, SearchPage, SearchRequest, SearchScope, Title,
};

use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};

const BRIEF: [(&str, &str); 1] = [("brief.txt", "Clause 4 draft\n")];

fn request(text: &str) -> SearchRequest {
    SearchRequest {
        query: text.to_owned(),
        scope: SearchScope::All,
        include_archived: true,
        kinds: None,
        limit: None,
        cursor: None,
        generation: Count::new(0).unwrap(),
    }
}

fn page(host: &Running, text: &str) -> SearchPage {
    match wait(
        "a search",
        host.handle
            .search(Principal::LocalDesktop, "r", request(text)),
    )
    .unwrap()
    {
        SearchAnswer::Page(page) => page,
        SearchAnswer::Cancelled => panic!("cancelled"),
    }
}

fn rename(host: &Running, thread: clauseo_protocol::ThreadId, title: &str) {
    host.ok(HostCommand::RenameThread {
        thread,
        title: Title::new(title).unwrap(),
        placeholder: false,
    });
}

#[test]
fn review_a_thread_made_just_before_forget_stays_out_of_search() {
    let dir = Dir::new(
        "search_review_forget",
        &[(FAKE, Scenario::Bundled("files-one-turn"))],
    );
    let hold = SearchHold::new();
    let held = hold.clone();
    let host = dir.start_with(0, move |config| {
        config
            .with_search_channel_capacity(1)
            .with_search_holds(None, Some(held))
    });
    let (matter, _, _) = matter_with_thread(&dir, &host, "Sharma lease", &BRIEF, FAKE);
    let (_, other, _) = matter_with_thread(&dir, &host, "Other", &BRIEF, FAKE);
    let handle = host.handle.clone();
    let first = thread::spawn(move || {
        wait(
            "the held search",
            handle.search(Principal::LocalDesktop, "w", request("x")),
        )
    });
    wait("the query hold", hold.reached());
    // The one slot of the channel: another matter's commit. The rest lag.
    rename(&host, other, "Firstfill");
    let late = host.create_thread(matter, FAKE);
    rename(&host, late, "Zebracrossing");
    host.ok(HostCommand::ForgetMatter { matter });
    hold.release();
    assert!(first.join().unwrap().is_ok());
    rename(&host, other, "Markerword");
    wait("the marker", async {
        loop {
            if let SearchAnswer::Page(found) = host
                .handle
                .search(Principal::LocalDesktop, "p", request("Markerword "))
                .await
                .unwrap()
                && !found.hits.is_empty()
            {
                return;
            }
        }
    });
    // The walks the lag queued run between searches: search a while (no sleeps) to give them
    // their turn; a forgotten matter's thread must never show.
    for _ in 0..200 {
        let leaked = page(&host, "Zebracrossing ");
        assert_eq!(
            leaked.hits,
            [],
            "a forgotten matter's thread shows in search"
        );
    }
    host.shutdown();
}
