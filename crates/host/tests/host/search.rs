//! Search (decision 0043) through the host: each kind found once its events commit, undo,
//! archive, a queued prompt's edit, a rebuild from the log, a lagging channel's catch-up, a
//! superseded search, forget and re-import, a garbled cursor, and a start-up that does not wait
//! for the index.

use std::thread;

use clauseo_host::SearchHold;
use clauseo_protocol::{
    CommandOutcome, Count, FileChoice, FileDecision, HostCommand, HostQuery, MatterId, MatterPath,
    Principal, QueryResult, SearchAnswer, SearchHit, SearchKind, SearchPage, SearchRequest,
    SearchScope, StopReason, StreamId, ThreadId, Title,
};

use crate::permissions::until_first_request;
use crate::queue::until_queued;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};
use crate::write;

const BRIEF: [(&str, &str); 2] = [("brief.txt", "Clause 4 draft\n"), ("old.txt", "old\n")];

const ANSWER: &str = "Created one note, edited the brief and deleted the old file.";

fn request(text: &str, generation: u64) -> SearchRequest {
    SearchRequest {
        query: text.to_owned(),
        scope: SearchScope::All,
        include_archived: false,
        kinds: None,
        limit: None,
        cursor: None,
        generation: Count::new(generation).unwrap(),
    }
}

fn kinds(text: &str, kinds: &[SearchKind]) -> SearchRequest {
    SearchRequest {
        kinds: Some(kinds.to_vec()),
        ..request(text, 0)
    }
}

fn search(host: &Running, window: &str, request: SearchRequest) -> SearchAnswer {
    wait(
        "a search",
        host.handle.search(Principal::LocalDesktop, window, request),
    )
    .unwrap()
}

fn page(answer: SearchAnswer) -> SearchPage {
    match answer {
        SearchAnswer::Page(page) => page,
        SearchAnswer::Cancelled => panic!("cancelled"),
    }
}

/// Searches until `done` holds of the page (the index follows commits on its own thread); the
/// watchdog fails the test if it never does.
fn poll(host: &Running, request: &SearchRequest, done: impl Fn(&SearchPage) -> bool) -> SearchPage {
    wait("the index to get there", async {
        loop {
            let answer = host
                .handle
                .search(Principal::LocalDesktop, "poll", request.clone())
                .await
                .unwrap();
            let found = page(answer);
            if done(&found) {
                return found;
            }
        }
    })
}

fn of_kind(page: &SearchPage, kind: SearchKind) -> Vec<&SearchHit> {
    page.hits.iter().filter(|hit| hit.kind == kind).collect()
}

fn snippet(hit: &SearchHit) -> String {
    hit.snippet.iter().map(|part| part.text.as_str()).collect()
}

/// A matter named "Sharma lease" with a thread titled "Rent dispute" whose one turn (prompt
/// "Draft the indemnity note") wrote notes/a.txt, edited brief.txt and deleted old.txt.
fn one_turn(dir: &Dir, host: &Running) -> (MatterId, ThreadId, clauseo_protocol::TurnId) {
    let (matter, thread, _) = matter_with_thread(dir, host, "Sharma lease", &BRIEF, FAKE);
    host.ok(HostCommand::RenameThread {
        thread,
        title: Title::new("Rent dispute").unwrap(),
    });
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Draft the indemnity note");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    (matter, thread, turn)
}

fn files_dir(test: &str) -> Dir {
    Dir::new(test, &[(FAKE, Scenario::Bundled("files-one-turn"))])
}

#[test]
fn each_kind_is_found_once_committed_and_an_answer_whole() {
    let dir = files_dir("search_each_kind");
    let host = dir.start(0);
    let (matter, thread, _) = one_turn(&dir, &host);
    let found = poll(&host, &request("Sharma ", 0), |p| {
        !of_kind(p, SearchKind::MatterTitle).is_empty()
    });
    assert_eq!(of_kind(&found, SearchKind::MatterTitle)[0].matter, matter);
    let found = poll(&host, &request("dispute ", 0), |p| {
        !of_kind(p, SearchKind::ThreadTitle).is_empty()
    });
    assert_eq!(
        of_kind(&found, SearchKind::ThreadTitle)[0].thread,
        Some(thread)
    );
    let found = poll(&host, &request("indemnity", 0), |p| {
        !of_kind(p, SearchKind::Prompt).is_empty()
    });
    let prompt = of_kind(&found, SearchKind::Prompt)[0];
    assert_eq!(prompt.thread_title.as_deref(), Some("Rent dispute"));
    assert_eq!(prompt.matter_title.as_deref(), Some("Sharma lease"));
    let found = poll(&host, &request("deleted ", 0), |p| {
        !of_kind(p, SearchKind::Answer).is_empty()
    });
    assert_eq!(snippet(of_kind(&found, SearchKind::Answer)[0]), ANSWER);
    let tools = poll(&host, &kinds("brief ", &[SearchKind::ToolTitle]), |p| {
        !p.hits.is_empty()
    });
    assert_eq!(snippet(&tools.hits[0]), "Edit brief.txt");
    let names = poll(&host, &kinds("notes ", &[SearchKind::FileName]), |p| {
        !p.hits.is_empty()
    });
    assert_eq!(names.hits[0].path.as_deref(), Some("notes/a.txt"));
    // old.txt was listed at the import and is gone from the checkpoint after the turn.
    poll(&host, &kinds("old ", &[SearchKind::FileName]), |p| {
        p.hits.is_empty()
    });
    host.shutdown();
}

#[test]
fn an_undone_step_takes_its_rows_and_archive_hides_them_unless_asked() {
    let dir = files_dir("search_undo_archive");
    let host = dir.start(0);
    let (_, thread, turn) = one_turn(&dir, &host);
    poll(&host, &request("indemnity ", 0), |p| p.hits.len() == 1);
    let outcome = host.ok(HostCommand::UndoStep {
        thread,
        turn,
        files: vec![FileDecision {
            path: MatterPath::new("notes/a.txt").unwrap(),
            choice: FileChoice::Undo,
        }],
    });
    assert!(
        matches!(outcome, CommandOutcome::StepUndone { .. }),
        "{outcome:?}"
    );
    for text in ["indemnity ", "deleted ", "Edit "] {
        poll(&host, &request(text, 0), |p| p.hits.is_empty());
    }
    // The title stays; archiving hides it unless archived conversations are asked for.
    poll(&host, &request("dispute ", 0), |p| p.hits.len() == 1);
    host.ok(HostCommand::ArchiveThread { thread });
    poll(&host, &request("dispute ", 0), |p| p.hits.is_empty());
    let archived = SearchRequest {
        include_archived: true,
        ..request("dispute ", 0)
    };
    assert_eq!(page(search(&host, "main", archived)).hits.len(), 1);
    host.shutdown();
}

#[test]
fn an_edited_queued_prompt_replaces_its_row() {
    let dir = Dir::new(
        "search_queued_edit",
        &[(FAKE, Scenario::Bundled("queue-two-prompts"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &BRIEF, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    host.prompt(thread, "Check clause 4.");
    until_first_request(&mut events);
    let (second, _) = host.prompt(thread, "Draft question");
    until_queued(&mut events, second);
    poll(&host, &request("Draft ", 0), |p| p.hits.len() == 1);
    host.ok(HostCommand::EditQueuedPrompt {
        thread,
        turn: second,
        text: "Second question".to_owned(),
    });
    let found = poll(&host, &request("Second ", 0), |p| p.hits.len() == 1);
    assert_eq!(found.hits[0].kind, SearchKind::Prompt);
    assert_eq!(page(search(&host, "main", request("Draft ", 0))).hits, []);
}

/// The hits a query gives, without their order (a rebuild inserts conversations by their last
/// activity, so equal times may come in another order).
fn hit_set(host: &Running, text: &str) -> Vec<String> {
    let mut hits: Vec<String> = page(search(host, "main", request(text, 0)))
        .hits
        .iter()
        .map(|hit| format!("{hit:?}"))
        .collect();
    hits.sort();
    hits
}

const QUERIES: [&str; 5] = ["Sharma ", "dispute ", "indemnity ", "deleted ", "brief "];

#[test]
fn a_rebuild_from_the_log_gives_the_same_hits_and_start_up_does_not_wait_for_it() {
    let dir = files_dir("search_rebuild");
    let host = dir.start(0);
    let (matter, _, _) = one_turn(&dir, &host);
    poll(&host, &request("deleted ", 0), |p| p.hits.len() == 1);
    poll(&host, &kinds("brief ", &[SearchKind::FileName]), |p| {
        !p.hits.is_empty()
    });
    let before: Vec<Vec<String>> = QUERIES.iter().map(|text| hit_set(&host, text)).collect();
    host.shutdown();
    for name in ["search.sqlite", "search.sqlite-wal", "search.sqlite-shm"] {
        let path = dir.data_dir().join(name);
        if path.exists() {
            write::remove_file(&path);
        }
    }
    let hold = SearchHold::new();
    let held = hold.clone();
    let host = dir.start_with(1, move |config| config.with_search_holds(Some(held), None));
    wait("the rebuild's hold", hold.reached());
    // The host serves while the rebuild waits: a command, and a search with its progress.
    let created = host.create_thread(matter, FAKE);
    assert!(host.threads(matter).iter().any(|t| t.thread == created));
    let waiting = page(search(&host, "main", request("deleted ", 0)));
    let progress = waiting.indexing.expect("a rebuild in progress");
    assert_eq!(progress.done, 0, "{waiting:?}");
    assert!(progress.total >= 1, "{waiting:?}");
    assert_eq!(waiting.hits, [], "{waiting:?}");
    hold.release();
    poll(&host, &request("deleted ", 0), |p| p.indexing.is_none());
    let after: Vec<Vec<String>> = QUERIES.iter().map(|text| hit_set(&host, text)).collect();
    assert_eq!(after, before);
    host.shutdown();
}

#[test]
fn a_lagging_channel_catches_up_from_the_store() {
    let dir = files_dir("search_lag");
    let hold = SearchHold::new();
    let held = hold.clone();
    let host = dir.start_with(0, move |config| {
        config
            .with_search_channel_capacity(1)
            .with_search_holds(None, Some(held))
    });
    // The thread stops at the first search; a whole turn's commits overflow the channel.
    let handle = host.handle.clone();
    let first = thread::spawn(move || {
        wait(
            "the held search",
            handle.search(Principal::LocalDesktop, "lag", request("x", 1)),
        )
    });
    wait("the query hold", hold.reached());
    one_turn(&dir, &host);
    hold.release();
    assert!(first.join().unwrap().is_ok());
    poll(&host, &request("indemnity ", 0), |p| p.hits.len() == 1);
    let found = poll(&host, &request("deleted ", 0), |p| p.hits.len() == 1);
    assert_eq!(snippet(&found.hits[0]), ANSWER);
    host.shutdown();
}

#[test]
fn a_newer_search_cancels_older_ones_and_a_late_answer_is_dropped() {
    let dir = files_dir("search_supersede");
    let hold = SearchHold::new();
    let held = hold.clone();
    let host = dir.start_with(0, move |config| config.with_search_holds(None, Some(held)));
    let handle = host.handle.clone();
    let running = thread::spawn(move || {
        wait(
            "the first search",
            handle.search(Principal::LocalDesktop, "main", request("Scratch", 1)),
        )
    });
    wait("the query hold", hold.reached());
    let handle = &host.handle;
    let (queued, newest, ()) = wait(
        "the queued searches",
        futures::future::join3(
            handle.search(Principal::LocalDesktop, "main", request("Scratch", 2)),
            handle.search(Principal::LocalDesktop, "main", request("Scratch", 3)),
            async {
                // Sent after both searches: once answered, both are queued.
                handle
                    .query(Principal::LocalDesktop, HostQuery::ListMatters)
                    .await
                    .unwrap();
                // An older generation than the newest is answered at once.
                let stale = handle
                    .search(Principal::LocalDesktop, "main", request("Scratch", 1))
                    .await;
                assert_eq!(stale, Ok(SearchAnswer::Cancelled));
                hold.release();
            },
        ),
    );
    assert_eq!(running.join().unwrap(), Ok(SearchAnswer::Cancelled));
    assert_eq!(queued, Ok(SearchAnswer::Cancelled));
    let newest = page(newest.unwrap());
    assert!(
        newest
            .hits
            .iter()
            .any(|hit| hit.kind == SearchKind::MatterTitle)
    );
    assert!(matches!(
        wait(
            "a query search",
            host.handle.query(
                Principal::LocalDesktop,
                HostQuery::Search(request("Scratch", 1))
            )
        ),
        Ok(QueryResult::Search(SearchAnswer::Page(_)))
    ));
    host.shutdown();
}

#[test]
fn a_forgotten_matter_leaves_search_and_its_import_brings_it_back() {
    let dir = files_dir("search_forget");
    let host = dir.start(0);
    let (matter, _, _) = one_turn(&dir, &host);
    poll(&host, &request("indemnity ", 0), |p| p.hits.len() == 1);
    host.ok(HostCommand::ForgetMatter { matter });
    for text in ["indemnity ", "Sharma ", "dispute "] {
        poll(&host, &request(text, 0), |p| p.hits.is_empty());
    }
    let back = host.create_matter("Sharma lease", &dir.work_path("Sharma lease"));
    assert_eq!(back, matter);
    let found = poll(&host, &request("indemnity ", 0), |p| p.hits.len() == 1);
    assert_eq!(found.hits[0].matter, matter);
    poll(&host, &request("deleted ", 0), |p| p.hits.len() == 1);
    host.shutdown();
}

#[test]
fn a_garbled_cursor_gives_the_first_page() {
    let dir = files_dir("search_cursor");
    let host = dir.start(0);
    one_turn(&dir, &host);
    let first = poll(&host, &request("brief ", 0), |p| p.hits.len() >= 2);
    for cursor in ["not a cursor", "AAAA", ""] {
        let garbled = SearchRequest {
            cursor: Some(cursor.to_owned()),
            ..request("brief ", 0)
        };
        assert_eq!(
            page(search(&host, "main", garbled)).hits,
            first.hits,
            "{cursor:?}"
        );
    }
    host.shutdown();
}
