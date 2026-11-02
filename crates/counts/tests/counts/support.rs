//! What every test uses: a clock in UTC, the app's envelope, a fake `Upload` that records each
//! body and answers from a script, and readers for the counts folder.

use std::collections::VecDeque;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use clauseo_counts::schema::{
    AppOpened, Arch, Batch, Channel, Day, Event, InstallType, Language, Launch, Platform, Time,
    Version,
};
use clauseo_counts::{About, Counts, Upload, UploadError, UploadPath, send_due};
use futures::future::BoxFuture;

pub(crate) use crate::write::{Scratch, plant};

pub(crate) const HOUR: u64 = 3600;
pub(crate) const DAY: u64 = 86_400;
pub(crate) const SEED: u64 = 7;

/// `2026-11-<d>` (November 2026; the 2nd is a Monday), `secs` seconds after its midnight UTC.
pub(crate) fn at(d: i64, secs: u64) -> Time {
    day(d).start().plus(secs)
}

/// `2026-11-<d>`, or the day `d - 30` of December past the 30th.
pub(crate) fn day(d: i64) -> Day {
    Day::from_ymd(2026, 11, 1)
        .unwrap()
        .days_before(1 - d)
        .unwrap()
}

pub(crate) fn about() -> About {
    About {
        version: "3.0.4".parse::<Version>().unwrap(),
        channel: Channel::Stable,
        platform: Platform::macos(15).unwrap(),
        arch: Arch::Aarch64,
        language: Language::En,
        install_type: InstallType::PerUser,
        managed: false,
    }
}

pub(crate) const COLD: Event = Event::AppOpened(AppOpened {
    launch: Launch::Cold,
});

/// Counts after the first act, with counts on.
pub(crate) fn saving(scratch: &Scratch) -> Counts {
    Counts::open(scratch.path(), about(), true, true, SEED)
}

pub(crate) fn counts_dir(scratch: &Scratch) -> PathBuf {
    scratch.path().join("counts")
}

/// The files under `dir`, as `/`-separated paths from it, sorted; none when it is missing.
pub(crate) fn files(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let entry = entry.unwrap();
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), &format!("{name}/"), out);
            } else {
                out.push(name);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out.sort();
    out
}

/// Bumps `COLD` on each of the days `from..=to` and ticks across each midnight, so each of them
/// is queued (closed) once `to + 1` begins.
pub(crate) fn fill_days(counts: &Counts, from: i64, to: i64) {
    for d in from..=to {
        counts.tick(at(d, 12 * HOUR)).unwrap();
        counts.bump(COLD);
    }
    counts.tick(at(to + 1, 0)).unwrap();
}

/// One POST the fake saw.
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    pub(crate) path: UploadPath,
    pub(crate) body: Vec<u8>,
}

/// A fake `Upload`: answers each POST with the next scripted answer (then the last one again),
/// and records each body.
pub(crate) struct FakeUpload {
    answers: Mutex<VecDeque<Result<u16, UploadError>>>,
    seen: Mutex<Vec<Seen>>,
}

impl FakeUpload {
    pub(crate) fn answering(answers: &[Result<u16, UploadError>]) -> FakeUpload {
        FakeUpload {
            answers: Mutex::new(answers.iter().copied().collect()),
            seen: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Upload for FakeUpload {
    fn post(
        &self,
        path: UploadPath,
        gzip_body: Vec<u8>,
    ) -> BoxFuture<'_, Result<u16, UploadError>> {
        self.seen.lock().unwrap().push(Seen {
            path,
            body: gzip_body,
        });
        let mut answers = self.answers.lock().unwrap();
        let answer = if answers.len() > 1 {
            answers.pop_front().unwrap()
        } else {
            *answers.front().unwrap()
        };
        Box::pin(async move { answer })
    }
}

/// `send_due` at `now`, with counts on.
pub(crate) fn send(counts: &Counts, upload: &FakeUpload, now: Time) {
    futures::executor::block_on(send_due(counts, upload, now, &|| true));
}

/// Ticks and sends every `step` seconds from `from` to `to`; returns when each POST went, with
/// what it carried.
pub(crate) fn run(
    counts: &Counts,
    upload: &FakeUpload,
    from: Time,
    to: Time,
    step: u64,
) -> Vec<(Time, Seen)> {
    let mut sent = Vec::new();
    let mut now = from;
    while now <= to {
        counts.tick(now).unwrap();
        let before = upload.seen().len();
        send(counts, upload, now);
        sent.extend(
            upload
                .seen()
                .into_iter()
                .skip(before)
                .map(|seen| (now, seen)),
        );
        now = now.plus(step);
    }
    sent
}

pub(crate) fn gunzip(body: &[u8]) -> Vec<u8> {
    assert_eq!(&body[..2], &[0x1f, 0x8b], "a gzip body");
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(body)
        .read_to_end(&mut json)
        .unwrap();
    json
}

pub(crate) fn batch_of(seen: &Seen) -> Batch {
    assert_eq!(seen.path, UploadPath::Counts);
    serde_json::from_slice(&gunzip(&seen.body)).unwrap()
}
