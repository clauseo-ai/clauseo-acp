//! The measurement behind the PR's Performance section, not a timing assertion: builds the
//! synthetic corpus (5,000 conversations, or `SEARCH_PERF_CONVERSATIONS`) and prints p50 and p95
//! of each query kind, per-entry insert and the deletes. Ignored by default; run it with
//! `cargo nextest run -p clauseo-search --cargo-profile preview --run-ignored only perf`.

#![expect(
    clippy::disallowed_methods,
    reason = "a measurement reads the clock; nothing else in the tests does"
)]

use std::time::{Duration, Instant};

use clauseo_search::{EntryKind, Mode, Query, SearchIndex};

use crate::common::{TestDir, at, matter, query, seq, thread};
use crate::corpus::{Item, Lang, Rng, SPAN_MINUTES, Vocab, generate, text};

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// p50, p95 and the largest, in milliseconds.
fn spread(mut times: Vec<Duration>) -> (f64, f64, f64) {
    times.sort();
    let at = |p: usize| ms(times[(times.len() - 1) * p / 100]);
    (at(50), at(95), at(100))
}

fn put(index: &mut SearchIndex, items: &[Item]) {
    index
        .batch(|b| {
            for item in items {
                match item {
                    Item::Matter {
                        matter: m,
                        at: t,
                        title,
                        files,
                    } => {
                        b.put_matter_title(matter(*m), at(*t), title)?;
                        b.put_file_names(matter(*m), at(*t), files)?;
                    }
                    Item::Thread {
                        matter: m,
                        thread: t,
                        at: when,
                        title,
                    } => {
                        b.put_thread_title(matter(*m), thread(*t), at(*when), title)?;
                        b.conversation_done();
                    }
                    Item::Entry {
                        matter: m,
                        thread: t,
                        entry,
                        prompt,
                        at: when,
                        body,
                    } => {
                        let kind = if *prompt {
                            EntryKind::Prompt
                        } else {
                            EntryKind::Answer
                        };
                        b.put_entry(matter(*m), thread(*t), seq(*entry), kind, at(*when), body)?;
                    }
                }
            }
            Ok(())
        })
        .unwrap();
}

fn categories(v: &Vocab) -> Vec<(&'static str, Mode, Vec<String>)> {
    let s = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
    vec![
        (
            "one common word",
            Mode::Ranked,
            s(&["tenant ", "agreement ", "notice ", "cheque ", "bail "]),
        ),
        (
            "one rare word",
            Mode::Ranked,
            (0..8)
                .map(|i| format!("{} ", v.names_latin[15_000 + i * 97]))
                .collect(),
        ),
        (
            "two words",
            Mode::Ranked,
            s(&[
                "tenant notice ",
                "cheque dishonour ",
                "interim relief ",
                "written statement ",
            ]),
        ),
        (
            "Devanagari word",
            Mode::Ranked,
            s(&["किरायेदार ", "न्यायालय ", "अनुबंध ", "धारा "]),
        ),
        (
            "Hinglish word",
            Mode::Ranked,
            s(&["kirayedar ", "adalat ", "zamanat "]),
        ),
        (
            "citation phrase",
            Mode::Ranked,
            s(&[
                "(2019) 5 SCC ",
                "Order XXXIX Rule 1 ",
                "s. 138 ",
                "AIR 1978 SC ",
            ]),
        ),
        (
            "prefix, 2 chars",
            Mode::Typing,
            s(&["ag", "te", "ev", "ch"]),
        ),
        (
            "prefix, 3 chars",
            Mode::Typing,
            s(&["agr", "ten", "evi", "arb"]),
        ),
        (
            "prefix, 5 chars",
            Mode::Typing,
            s(&["agree", "tenan", "evict", "arbit"]),
        ),
        (
            "Devanagari prefix",
            Mode::Typing,
            s(&["अनु", "किराय", "न्याय"]),
        ),
        (
            "two words, typing",
            Mode::Typing,
            s(&["tenant noti", "cheque dish"]),
        ),
    ]
}

#[test]
#[ignore = "a measurement: run with --run-ignored only, in an optimised profile"]
fn perf() {
    let n: u64 = std::env::var("SEARCH_PERF_CONVERSATIONS").map_or(5_000, |v| v.parse().unwrap());
    let dir = TestDir::new("perf");
    let (mut index, _) = SearchIndex::open(dir.path()).unwrap();
    let mut items = Vec::new();
    generate(n, |item| items.push(item));
    index.start_rebuild(n).unwrap();
    let started = Instant::now();
    for chunk in items.chunks(5_000) {
        put(&mut index, chunk);
    }
    let build = started.elapsed();
    let started = Instant::now();
    index.finish_rebuild().unwrap();
    let optimize = started.elapsed();
    let bytes = std::fs::metadata(dir.path().join(clauseo_search::FILE_NAME))
        .unwrap()
        .len();
    println!(
        "conversations {n}, rows put {}, build {build:.2?}, optimize {optimize:.2?}, file {:.1} MB",
        items.len(),
        f64::from(u32::try_from(bytes / 1000).unwrap()) / 1e3
    );

    let v = Vocab::new();
    let mut all = [Vec::new(), Vec::new()];
    println!(
        "{:<20} {:>6} {:>9} {:>9}",
        "query", "hits", "p50 ms", "p95 ms"
    );
    for (name, mode, texts) in categories(&v) {
        let mut times = Vec::new();
        let mut hits = 0;
        for text in &texts {
            let q = Query {
                limit: 20,
                now: at(SPAN_MINUTES + 1_440),
                ..query(text, mode)
            };
            for _ in 0..5 {
                let started = Instant::now();
                let page = index.query(&q).unwrap();
                times.push(started.elapsed());
                hits = hits.max(page.hits.len());
            }
        }
        all[usize::from(mode == Mode::Ranked)].extend(times.iter().copied());
        let (p50, p95, _) = spread(times);
        println!("{name:<20} {hits:>6} {p50:>9.2} {p95:>9.2}");
    }
    let (p50, p95, max) = spread(all[0].clone());
    println!("all Typing queries: p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms");
    let (p50, p95, max) = spread(all[1].clone());
    println!("all Ranked queries: p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms");

    let mut r = Rng::new(99);
    let mut inserts = Vec::new();
    for k in 0..300 {
        let lang = if k % 5 == 0 { Lang::Hi } else { Lang::En };
        let body = text(&mut r, &v, lang, 3_000);
        let started = Instant::now();
        index
            .put_entry(
                matter(0),
                thread(n + 1),
                seq(k + 1),
                EntryKind::Answer,
                at(SPAN_MINUTES + i64::try_from(k).unwrap()),
                &body,
            )
            .unwrap();
        inserts.push(started.elapsed());
    }
    let (p50, p95, max) = spread(inserts);
    println!("insert one 3,000-char answer: p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms");
    let started = Instant::now();
    index.delete_thread(thread(5)).unwrap();
    println!(
        "delete one thread (21 rows), secure-delete and checkpoint: {:.1} ms",
        ms(started.elapsed())
    );
    let started = Instant::now();
    index.delete_matter(matter(1)).unwrap();
    println!(
        "delete one matter, secure-delete and checkpoint: {:.1} ms",
        ms(started.elapsed())
    );
}
