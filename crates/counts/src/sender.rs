//! The sender (research 37 §4.1): which POST is due, its body, and what its answer does.
//!
//! Days go once a UTC day, 0 to 10 minutes after the first tick and after each midnight: the 7
//! oldest closed days, cut until the gzip body is at most 16 KB. A failed try is retried 1 h,
//! 3 h, 7 h, 11 h, 15 h, 19 h and 23 h after the first (gaps of 1 h, 2 h, then 4 h), and after
//! that the days wait for the next daily send. A 2xx or a `400` takes the days out of the queue;
//! anything else is a failed try. A tally goes alone, at its random moment in the week after its
//! week, never within an hour of a day try, with the same retries. One POST is in flight at most;
//! one with no answer after 10 minutes is a failed try.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::pin::Pin;
use std::task::{Context, Poll};

use flate2::Compression;
use flate2::write::GzEncoder;
use futures::channel::oneshot;

use crate::Mode;
use crate::random::Rng;
use crate::schema::{
    Batch, DAY_SECS, Day, MAX_COUNTS_GZIP, MAX_COUNTS_JSON, MAX_DAY_AGE, MAX_DAYS_PER_BATCH,
    MAX_TALLY_GZIP, MAX_TALLY_JSON, Time, Week,
};
use crate::state::{State, Tally};
use crate::upload::UploadPath;

/// When each retry comes, counted from a round's first try.
const RETRY_AT_SECS: [u64; 7] = [
    3600,
    3 * 3600,
    7 * 3600,
    11 * 3600,
    15 * 3600,
    19 * 3600,
    23 * 3600,
];
/// The longest wait before the daily send: 0 to 600 seconds.
const MAX_DELAY_SECS: u64 = 600;
/// A POST with no answer after this long is a failed try.
const STALE_SECS: u64 = 600;
/// A tally never goes within this long of a day try, before or after.
const TALLY_GAP_SECS: u64 = 3600;

/// One POST's number, to match its answer to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PostId(u64);

/// How a POST ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The server answered with this status.
    Status(u16),
    /// No answer: the network failed or timed out.
    Failed,
}

/// A POST to make: the path, the gzip body, and a future that ends when counts turn off, which
/// must cancel the request (it is never retried).
#[derive(Debug)]
pub struct Post {
    id: PostId,
    path: UploadPath,
    body: Vec<u8>,
    cancel: Cancelled,
}

impl Post {
    /// The number to pass back to [`crate::Counts::sent`].
    pub fn id(&self) -> PostId {
        self.id
    }

    /// Where it goes.
    pub fn path(&self) -> UploadPath {
        self.path
    }

    /// The gzip body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Its parts, to send it.
    pub fn into_parts(self) -> (PostId, UploadPath, Vec<u8>, Cancelled) {
        (self.id, self.path, self.body, self.cancel)
    }
}

/// Ends when counts turn off (or are dropped) while its POST is in flight.
#[derive(Debug)]
pub struct Cancelled(oneshot::Receiver<()>);

impl Future for Cancelled {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        Pin::new(&mut self.0).poll(cx).map(|_| ())
    }
}

/// A run of tries for one payload.
#[derive(Debug, Clone, Copy)]
struct Round {
    first: Time,
    tries: usize,
    next: Time,
}

impl Round {
    fn start(now: Time) -> Round {
        Round {
            first: now,
            tries: 0,
            next: now,
        }
    }

    /// After a failed try: the round with its next try, or `None` once 24 hours are used up.
    fn failed(self) -> Option<Round> {
        let at = RETRY_AT_SECS.get(self.tries.checked_sub(1)?)?;
        Some(Round {
            next: self.first.plus(*at),
            ..self
        })
    }
}

/// What the POST in flight carries.
#[derive(Debug)]
enum What {
    Days(Vec<Day>),
    Tally(Week),
}

#[derive(Debug)]
struct InFlight {
    id: PostId,
    what: What,
    started: Time,
    /// Dropped to cancel the POST.
    _cancel: oneshot::Sender<()>,
}

/// The sender's own state. Resetting it (counts off) cancels the POST in flight.
#[derive(Debug, Default)]
pub(crate) struct Sender {
    /// When the next daily send may go; set at the first tick.
    next_daily: Option<Time>,
    days: Option<Round>,
    tallies: BTreeMap<Week, Round>,
    in_flight: Option<InFlight>,
    /// The next POST's number. It keeps counting across off, so post numbers never repeat in a
    /// process and a late answer to a POST from before off cannot match one made after.
    next_id: u64,
    last_day_try: Option<Time>,
}

impl Sender {
    /// Counts off: drops every round and the POST in flight (which cancels it), and keeps
    /// counting post numbers.
    pub(crate) fn reset(&mut self) {
        *self = Sender {
            next_id: self.next_id,
            ..Sender::default()
        };
    }

    /// At the first tick: the daily send goes 0 to 10 minutes from now.
    pub(crate) fn start(&mut self, now: Time, rng: &mut Rng) {
        if self.next_daily.is_none() {
            self.next_daily = Some(now.plus(rng.below(MAX_DELAY_SECS + 1)));
        }
    }

    /// At a UTC midnight: the daily send goes 0 to 10 minutes after it, unless a round is on.
    pub(crate) fn new_day(&mut self, day: Day, rng: &mut Rng) {
        if self.next_daily.is_some() && self.days.is_none() {
            self.next_daily = Some(day.start().plus(rng.below(MAX_DELAY_SECS + 1)));
        }
    }

    fn post(&mut self, what: What, path: UploadPath, body: Vec<u8>, now: Time) -> Post {
        let id = PostId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        let (cancel_tx, cancel_rx) = oneshot::channel();
        self.in_flight = Some(InFlight {
            id,
            what,
            started: now,
            _cancel: cancel_tx,
        });
        Post {
            id,
            path,
            body,
            cancel: Cancelled(cancel_rx),
        }
    }

    /// The next daily send after a round that ended at `at`.
    fn next_day_after(&mut self, at: Time, rng: &mut Rng) {
        let tomorrow = at.day().map_or(at, |day| day.start().plus(DAY_SECS));
        self.next_daily = Some(tomorrow.plus(rng.below(MAX_DELAY_SECS + 1)));
    }
}

/// `bytes` gzipped, with no name and no time in the header.
fn gzip(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).ok()?;
    encoder.finish().ok()
}

/// The POST due at `now`, if any: none before the first act, none while one is in flight.
pub(crate) fn due(state: &mut State, now: Time) -> Option<Post> {
    expire_stale(state, now);
    if state.mode != Mode::Saving || state.sender.in_flight.is_some() {
        return None;
    }
    let today = now.day()?;
    if day_try_due(state, now, today)
        && let Some(post) = day_post(state, now, today)
    {
        return Some(post);
    }
    tally_post(state, now, today)
}

/// The days the next day send may hold: closed, at most 14 days old, oldest first, and made
/// under the current install id. A report is never given another id: one from before off (whose
/// delete failed) waits under its own id until the age rule drops it, and is never sent.
fn sendable(state: &State, today: Day) -> impl Iterator<Item = Day> + '_ {
    state
        .queue
        .iter()
        .filter(move |&(&day, report)| {
            (1..=MAX_DAY_AGE).contains(&today.days_since(day))
                && Some(report.install) == state.install
        })
        .map(|(&day, _)| day)
}

fn day_try_due(state: &State, now: Time, today: Day) -> bool {
    match state.sender.days {
        Some(round) => now >= round.next,
        None => {
            state.sender.next_daily.is_some_and(|at| at <= now)
                && state.today.posted != Some(today)
                && sendable(state, today).next().is_some()
        }
    }
}

/// A day try within the hour, which a tally must not come near.
fn day_try_soon(state: &State, now: Time, today: Day) -> bool {
    let soon = now.plus(TALLY_GAP_SECS);
    let recent = state
        .sender
        .last_day_try
        .is_some_and(|at| now.since(at) < TALLY_GAP_SECS);
    let next = match state.sender.days {
        Some(round) => round.next <= soon,
        None => {
            state.sender.next_daily.is_some_and(|at| at <= soon)
                && state.today.posted != Some(today)
                && sendable(state, today).next().is_some()
        }
    };
    recent || next
}

/// The body of the day send: the 7 oldest sendable days, the newest dropped until it fits.
/// A single day too large to fit is taken out of the queue.
fn day_body(state: &mut State, today: Day) -> Option<(Vec<Day>, Vec<u8>)> {
    loop {
        let mut days: Vec<Day> = sendable(state, today).take(MAX_DAYS_PER_BATCH).collect();
        while let Some(&last) = days.last() {
            let reports = days
                .iter()
                .filter_map(|day| state.queue.get(day).cloned())
                .collect();
            let json = serde_json::to_vec(&Batch { days: reports }).ok()?;
            let body = gzip(&json)?;
            if json.len() <= MAX_COUNTS_JSON && body.len() <= MAX_COUNTS_GZIP {
                return Some((days, body));
            }
            if days.len() == 1 {
                state.queue.remove(&last);
                state.forget_day(last);
                break;
            }
            days.pop();
        }
        if days.is_empty() && sendable(state, today).next().is_none() {
            return None;
        }
    }
}

fn day_post(state: &mut State, now: Time, today: Day) -> Option<Post> {
    let Some((days, body)) = day_body(state, today) else {
        state.sender.days = None;
        return None;
    };
    let mut round = state.sender.days.unwrap_or_else(|| Round::start(now));
    round.tries += 1;
    state.sender.days = Some(round);
    state.sender.last_day_try = Some(now);
    Some(
        state
            .sender
            .post(What::Days(days), UploadPath::Counts, body, now),
    )
}

fn tally_post(state: &mut State, now: Time, today: Day) -> Option<Post> {
    if day_try_soon(state, now, today) {
        return None;
    }
    let due: Vec<Week> = state
        .tallies
        .iter()
        .filter_map(|(&week, tally)| match tally {
            Tally::Closed { send_at, .. } => {
                let at = state
                    .sender
                    .tallies
                    .get(&week)
                    .map_or(*send_at, |round| round.next);
                (at <= now).then_some(week)
            }
            Tally::Open(_) => None,
        })
        .collect();
    for week in due {
        let Some(Tally::Closed { report, .. }) = state.tallies.get(&week) else {
            continue;
        };
        let json = serde_json::to_vec(report).ok()?;
        let body = gzip(&json)?;
        if json.len() > MAX_TALLY_JSON || body.len() > MAX_TALLY_GZIP {
            state.forget_tally(week);
            continue;
        }
        let round = state
            .sender
            .tallies
            .entry(week)
            .or_insert_with(|| Round::start(now));
        round.tries += 1;
        return Some(
            state
                .sender
                .post(What::Tally(week), UploadPath::Tally, body, now),
        );
    }
    None
}

/// A POST in flight for 10 minutes or more is a failed try.
fn expire_stale(state: &mut State, now: Time) {
    let stale = state
        .sender
        .in_flight
        .as_ref()
        .is_some_and(|flight| now.since(flight.started) >= STALE_SECS);
    if stale && let Some(flight) = state.sender.in_flight.take() {
        answered(state, flight, Outcome::Failed);
    }
}

/// The answer to POST `id`. An answer to a POST no longer in flight (cancelled by off, or
/// already counted as failed) changes nothing.
pub(crate) fn sent(state: &mut State, id: PostId, outcome: Outcome) {
    let ours = state
        .sender
        .in_flight
        .as_ref()
        .is_some_and(|flight| flight.id == id);
    if ours && let Some(flight) = state.sender.in_flight.take() {
        answered(state, flight, outcome);
    }
}

fn answered(state: &mut State, flight: InFlight, outcome: Outcome) {
    let taken = matches!(outcome, Outcome::Status(200..=299 | 400));
    match (flight.what, taken) {
        (What::Days(days), true) => {
            for day in days {
                state.queue.remove(&day);
                state.forget_day(day);
            }
            state.today.posted = flight.started.day();
            state.dirty = true;
            state.sender.days = None;
            state.sender.next_day_after(flight.started, &mut state.rng);
        }
        (What::Days(_), false) => {
            state.sender.days = state.sender.days.and_then(Round::failed);
            if state.sender.days.is_none() {
                state.sender.next_day_after(flight.started, &mut state.rng);
            }
        }
        (What::Tally(week), true) => {
            state.sender.tallies.remove(&week);
            state.forget_tally(week);
        }
        (What::Tally(week), false) => {
            let round = state.sender.tallies.remove(&week).and_then(Round::failed);
            if let Some(round) = round {
                state.sender.tallies.insert(week, round);
            } else if let Some(Tally::Closed { send_at, .. }) = state.tallies.get_mut(&week) {
                // The 24 hours are used up: a new random moment in the week after them.
                let wait = DAY_SECS + state.rng.below(7 * DAY_SECS);
                *send_at = flight.started.plus(wait);
                state.dirty = true;
            }
        }
    }
}
