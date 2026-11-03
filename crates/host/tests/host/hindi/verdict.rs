//! The Hindi question's pass rule, as plain functions over the answer's text: mainly
//! Devanagari, and the fact planted in the fixture papers named.

use std::fmt;

use unicode_normalization::UnicodeNormalization as _;

/// The fixture papers, in `tests/fixtures/hindi/` (`README.md` there says what each is).
pub(crate) const PAPERS: [&str; 4] = [
    "kirayanama.pdf",
    "notice-legacy.pdf",
    "affidavit-scan.pdf",
    "patra.docx",
];

/// The papers that hold the planted fact.
pub(crate) const PLANTED_IN: [&str; 2] = ["kirayanama.pdf", "patra.docx"];

/// The planted tenant's name, as `generate.py` writes it (NFC).
pub(crate) const TENANT: &str = "सुरेखा भार्गव";
/// The tenant's surname: an answer that names her names it, whatever it does with the rest.
pub(crate) const SURNAME: &str = "भार्गव";
/// The planted deposit as `kirayanama.pdf` writes it (`patra.docx` uses Devanagari digits).
pub(crate) const AMOUNT: &str = "3,47,215";
/// The planted lease date as `kirayanama.pdf` writes it.
pub(crate) const DATE: &str = "17 मार्च 2024";

/// The least share of an answer's letters that must be Devanagari (U+0900 to U+097F).
pub(crate) const MIN_DEVANAGARI: f64 = 0.6;

/// The question, in Hindi: who the tenant is, what the deposit was and when the lease was
/// signed, which only the papers say, and which paper says it.
pub(crate) const QUESTION: &str = "इस मामले के कागज़ात पढ़कर बताइए: किरायेदार का नाम क्या है, \
उसने कितनी जमानत राशि जमा की, और किरायानामा किस तारीख़ को हुआ? उत्तर हिंदी में दीजिए और \
बताइए कि यह जानकारी किस कागज़ में मिली।";

/// What the rule found in one answer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Verdict {
    /// The answer's letters (`char::is_alphabetic`, which counts Devanagari's vowel signs).
    pub(crate) letters: usize,
    /// Of those, the ones in U+0900 to U+097F.
    pub(crate) devanagari: usize,
    /// The tenant's surname appears (compared in NFC).
    pub(crate) name: bool,
    /// The deposit appears as one number, in ASCII or Devanagari digits, with or without commas.
    pub(crate) amount: bool,
    /// The fixture papers the answer names, by file name or by file name without extension.
    pub(crate) cited: Vec<&'static str>,
}

impl Verdict {
    /// Applies the rule to `answer`.
    pub(crate) fn of(answer: &str) -> Self {
        let answer: String = answer.nfc().collect();
        let letters = answer.chars().filter(|c| c.is_alphabetic()).count();
        let devanagari = answer
            .chars()
            .filter(|c| c.is_alphabetic() && ('\u{0900}'..='\u{097F}').contains(c))
            .count();
        let lower = answer.to_lowercase();
        let cited = PAPERS
            .into_iter()
            .filter(|paper| {
                let stem = paper.rsplit_once('.').map_or(*paper, |(stem, _)| stem);
                lower.contains(paper) || lower.contains(stem)
            })
            .collect();
        Self {
            letters,
            devanagari,
            name: answer.contains(SURNAME),
            amount: numbers(&answer).contains(&AMOUNT.replace(',', "")),
            cited,
        }
    }

    /// The share of letters that are Devanagari; 0 for an answer with no letters.
    pub(crate) fn ratio(&self) -> f64 {
        if self.letters == 0 {
            return 0.0;
        }
        // Answers are far shorter than 2^52 letters, so both convert exactly.
        let (devanagari, letters) = (
            u32::try_from(self.devanagari).unwrap_or(u32::MAX),
            u32::try_from(self.letters).unwrap_or(u32::MAX),
        );
        f64::from(devanagari) / f64::from(letters)
    }

    /// Mainly Devanagari, and the planted name or amount named.
    pub(crate) fn passed(&self) -> bool {
        self.ratio() >= MIN_DEVANAGARI && (self.name || self.amount)
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: Devanagari {}/{} letters = {:.2} (needs {MIN_DEVANAGARI}); tenant named: {}; \
             deposit named: {}; papers named: {:?}",
            if self.passed() { "PASS" } else { "FAIL" },
            self.devanagari,
            self.letters,
            self.ratio(),
            self.name,
            self.amount,
            self.cited,
        )
    }
}

/// `text`'s numbers: each run of digits and commas, commas dropped, Devanagari digits
/// (U+0966 to U+096F) as ASCII, so ₹3,47,215 and ₹३,४७,२१५ both give `347215`.
fn numbers(text: &str) -> Vec<String> {
    let ascii: String = text
        .chars()
        .map(|c| match c {
            '\u{0966}'..='\u{096F}' => char::from_digit(u32::from(c) - 0x0966, 10).unwrap_or(c),
            _ => c,
        })
        .collect();
    ascii
        .split(|c: char| !c.is_ascii_digit() && c != ',')
        .map(|run| run.replace(',', ""))
        .filter(|run| !run.is_empty())
        .collect()
}
