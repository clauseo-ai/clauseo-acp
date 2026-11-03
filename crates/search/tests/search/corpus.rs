//! A deterministic synthetic corpus for the measurement, from the search spike: matters with
//! 100 file names each, and conversations of 20 entries alternating prompt (200 to 400 chars)
//! and answer (1,500 to 4,500), 60% English, 20% Devanagari Hindi, 20% Hinglish, a quarter of
//! sentences with a citation.

use crate::words::{ACTS, EN, HI, HINGLISH, ROMAN};

pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub(crate) fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }

    /// Zipf-ish skew: low indices much more often; `n × u^2.5` for `u` uniform in [0, 1),
    /// in whole numbers (u is 20 bits, so u^2.5 is at most 2^50).
    fn zipf(&mut self, n: usize) -> usize {
        let u = u128::from(self.next() % (1 << 20));
        let power = u * u * u128::from(u64::try_from(u).unwrap().isqrt());
        usize::try_from((u128::try_from(n).unwrap() * power) >> 50).unwrap() % n
    }
}

fn citation(r: &mut Rng) -> String {
    match r.below(7) {
        0 => format!(
            "({}) {} SCC {}",
            1980 + r.below(45),
            1 + r.below(12),
            1 + r.below(900)
        ),
        1 => format!("AIR {} SC {}", 1950 + r.below(75), 1 + r.below(3000)),
        2 => format!(
            "s. {} of the {}",
            1 + r.below(500),
            ACTS[r.below(ACTS.len())]
        ),
        3 => format!("Section {} {}", 1 + r.below(500), ACTS[r.below(ACTS.len())]),
        4 => format!(
            "Order {} Rule {}",
            ROMAN[r.below(ROMAN.len())],
            1 + r.below(12)
        ),
        5 => format!(
            "{} SCC OnLine {} {}",
            2010 + r.below(16),
            ["Del", "Bom", "Mad", "Cal", "All"][r.below(5)],
            1 + r.below(9000)
        ),
        _ => format!(
            "धारा {} {}",
            1 + r.below(500),
            ["भारतीय दंड संहिता", "दंड प्रक्रिया संहिता", "अधिनियम"][r.below(3)]
        ),
    }
}

/// Synthetic names: 20k Latin, 5k Devanagari.
pub(crate) struct Vocab {
    pub(crate) names_latin: Vec<String>,
    names_deva: Vec<String>,
}

impl Vocab {
    pub(crate) fn new() -> Self {
        let mut r = Rng::new(7);
        let syl = [
            "ka", "ra", "sha", "ma", "ni", "de", "vi", "gu", "pta", "lal", "jee", "tri", "pa",
            "tho", "sin", "gh", "ver", "an", "dra", "mo", "hit", "esh", "pur", "bad", "nag",
        ];
        let names_latin = (0..20_000)
            .map(|_| {
                let n = 2 + r.below(3);
                (0..n).map(|_| syl[r.below(syl.len())]).collect::<String>()
            })
            .collect();
        let cons = [
            'क', 'ग', 'त', 'द', 'न', 'प', 'ब', 'म', 'र', 'ल', 'स', 'ह', 'श', 'ज', 'च',
        ];
        let matra = ["", "ा", "ि", "ी", "ु", "े", "ो", "्"];
        let names_deva = (0..5_000)
            .map(|_| {
                let n = 2 + r.below(3);
                let mut s = String::new();
                for i in 0..n {
                    s.push(cons[r.below(cons.len())]);
                    let m = matra[r.below(matra.len())];
                    if !(m == "्" && i == n - 1) {
                        s.push_str(m);
                    }
                }
                s
            })
            .collect();
        Self {
            names_latin,
            names_deva,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    En,
    Hi,
    Hinglish,
}

pub(crate) fn text(r: &mut Rng, v: &Vocab, lang: Lang, target_chars: usize) -> String {
    let mut s = String::with_capacity(target_chars * 2);
    let mut chars = 0usize;
    while chars < target_chars {
        let words = 8 + r.below(13);
        for i in 0..words {
            let w: &str = if r.below(100) < 6 {
                match lang {
                    Lang::Hi => &v.names_deva[r.zipf(v.names_deva.len())],
                    _ => &v.names_latin[r.zipf(v.names_latin.len())],
                }
            } else {
                match lang {
                    Lang::En => EN[r.zipf(EN.len())],
                    Lang::Hi if r.below(100) < 15 => EN[r.zipf(EN.len())],
                    Lang::Hi => HI[r.zipf(HI.len())],
                    Lang::Hinglish if r.below(100) < 40 => EN[r.zipf(EN.len())],
                    Lang::Hinglish => HINGLISH[r.zipf(HINGLISH.len())],
                }
            };
            if i > 0 {
                s.push(' ');
                chars += 1;
            }
            chars += w.chars().count();
            s.push_str(w);
        }
        if r.below(100) < 25 {
            let c = citation(r);
            chars += c.chars().count() + 4;
            s.push_str(", see ");
            s.push_str(&c);
        }
        s.push_str(if lang == Lang::Hi { "। " } else { ". " });
        chars += 2;
    }
    s
}

/// One generated row.
pub(crate) enum Item {
    Matter {
        matter: u64,
        at: i64,
        title: String,
        files: Vec<String>,
    },
    Thread {
        matter: u64,
        thread: u64,
        at: i64,
        title: String,
    },
    Entry {
        matter: u64,
        thread: u64,
        entry: u64,
        prompt: bool,
        at: i64,
        body: String,
    },
}

/// Minutes from the corpus's start to its end: two years.
pub(crate) const SPAN_MINUTES: i64 = 2 * 365 * 1_440;

/// `n` conversations in `n / 25` matters (at least one), oldest first; `at` in minutes.
pub(crate) fn generate(conversations: u64, mut sink: impl FnMut(Item)) {
    let vocab = Vocab::new();
    let mut rng = Rng::new(42);
    let matters = (conversations / 25).max(1);
    let span =
        |i: u64, of: u64| SPAN_MINUTES * i64::try_from(i).unwrap() / i64::try_from(of).unwrap();
    for m in 0..matters {
        let first = vocab.names_latin[rng.below(5000)].clone();
        let second = vocab.names_latin[rng.below(5000)].clone();
        let kind = [
            "rent dispute",
            "cheque bounce",
            "arbitration",
            "divorce",
            "property partition",
            "bail",
        ][rng.below(6)];
        let files = (0..100)
            .map(|f| {
                let doc = [
                    "Plaint",
                    "Written statement",
                    "Annexure P-3",
                    "Rent agreement",
                    "Legal notice",
                    "Reply",
                    "Affidavit",
                    "Order dated 12.03.2024",
                    "Vakalatnama",
                    "Bank statement",
                    "किरायानामा",
                    "Lease deed",
                ][rng.below(12)];
                let ext = ["docx", "pdf", "pdf", "pdf", "jpg", "xlsx"][rng.below(6)];
                format!("{doc} - {first} {f}.{ext}")
            })
            .collect();
        sink(Item::Matter {
            matter: m,
            at: span(m, matters),
            title: format!("{first} v {second} {kind}"),
            files,
        });
    }
    for t in 0..conversations {
        let matter = u64::try_from(rng.below(usize::try_from(matters).unwrap())).unwrap();
        let lang = match rng.below(10) {
            0 | 1 => Lang::Hi,
            2 | 3 => Lang::Hinglish,
            _ => Lang::En,
        };
        let mut at = span(t, conversations);
        let title: String = text(&mut rng, &vocab, lang, 25).chars().take(40).collect();
        sink(Item::Thread {
            matter,
            thread: t,
            at,
            title,
        });
        for e in 0..20 {
            let prompt = e % 2 == 0;
            let len = if prompt {
                200 + rng.below(200)
            } else {
                1500 + rng.below(3000)
            };
            at += 1 + i64::try_from(rng.below(5)).unwrap();
            let body = text(&mut rng, &vocab, lang, len);
            sink(Item::Entry {
                matter,
                thread: t,
                entry: e * 7 + 3,
                prompt,
                at,
                body,
            });
        }
    }
}
