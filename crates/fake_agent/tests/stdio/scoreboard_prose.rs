//! The prose of the `scoreboard` scenario: a Markdown legal memo for the answer, and a thought
//! block. Sentence templates and word lists, joined by a seeded PRNG, so the same seed always
//! gives the same text. Nothing here reads a file or the clock. Every character is ASCII except
//! the section sign `§`, which the memo's table uses.

/// The memo's target length in bytes. The sections are filled up to their share of it, so the
/// memo ends within one paragraph past this figure.
pub(crate) const ANSWER_BYTES: usize = 80_000;

/// The thought block is at least this long, in characters.
const THOUGHT_CHARS: usize = 600;

/// A splitmix64 generator: the same seed gives the same numbers on every platform.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n`, for `n` above zero.
    pub(crate) fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next_u64() % u64::try_from(n).unwrap()).unwrap()
    }

    /// A number from `lo` to `hi`, both included.
    pub(crate) fn between(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }

    /// One of `items`.
    fn pick(&mut self, items: &[&'static str]) -> &'static str {
        items[self.below(items.len())]
    }

    /// One topic of the memo, chosen at random.
    fn topic(&mut self) -> &'static Topic {
        &TOPICS[self.below(TOPICS.len())]
    }
}

/// The thought block: sentences until it is at least [`THOUGHT_CHARS`] long.
pub(crate) fn thought(rng: &mut Rng) -> String {
    let mut text = String::new();
    while text.chars().count() < THOUGHT_CHARS {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(rng.pick(THOUGHTS));
    }
    text
}

/// The answer: a memo with one `#` title, thirteen `##` sections, bullet and numbered lists, a
/// table, and inline bold. Sections are filled in order up to their share of [`ANSWER_BYTES`].
pub(crate) fn answer(rng: &mut Rng) -> String {
    let mut memo = String::from(TITLE);
    push_block(&mut memo, INTRO);
    for (index, (heading, extra)) in SECTIONS.iter().enumerate() {
        let target = ANSWER_BYTES * (index + 1) / SECTIONS.len();
        push_block(&mut memo, &format!("## {heading}"));
        push_block(&mut memo, &paragraph(rng));
        let block = match extra {
            Extra::Nothing => None,
            Extra::Definitions => Some(DEFINITIONS.join("\n")),
            Extra::Bullets(count) => Some(bullets(rng, *count)),
            Extra::Numbered(count) => Some(numbered(rng, *count)),
            Extra::Table => Some(TABLE.to_owned()),
        };
        if let Some(block) = block {
            push_block(&mut memo, &block);
        }
        while memo.len() < target {
            push_block(&mut memo, &paragraph(rng));
        }
    }
    memo
}

/// Appends `block` after a blank line, unless `text` is still empty.
fn push_block(text: &mut String, block: &str) {
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(block);
}

/// What a section has after its opening paragraph.
enum Extra {
    Nothing,
    /// The defined terms, one bullet each (section 3).
    Definitions,
    /// This many generated bullets.
    Bullets(usize),
    /// This many numbered recommendations.
    Numbered(usize),
    /// The fees table (section 5).
    Table,
}

const TITLE: &str = "# Memorandum: review of the master services agreement between Calder Estates Ltd and \
     Northwind Analytics Ltd";

const INTRO: &str = "This memorandum reviews the draft master services agreement between Calder \
     Estates Ltd (the Client) and Northwind Analytics Ltd (the Supplier). It is written for the \
     Client's general counsel and its commercial team, and it flags the points we would raise \
     before signature. Clause references are to the draft of 14 October, and Schedule B is the \
     draft service schedule. We have not seen the Supplier's standard terms, so our view is \
     provisional where the draft is silent.";

const SECTIONS: [(&str, Extra); 13] = [
    ("1. Purpose and scope", Extra::Nothing),
    ("2. Parties and background", Extra::Nothing),
    ("3. Definitions and structure", Extra::Definitions),
    ("4. Services and service levels", Extra::Numbered(3)),
    ("5. Charges and payment", Extra::Table),
    ("6. Liability and indemnities", Extra::Bullets(4)),
    ("7. Data protection and confidentiality", Extra::Nothing),
    ("8. Intellectual property", Extra::Nothing),
    ("9. Term, termination and exit", Extra::Bullets(3)),
    ("10. Dispute resolution and governing law", Extra::Nothing),
    ("11. Negotiation priorities", Extra::Numbered(4)),
    ("12. Recommendations", Extra::Numbered(5)),
    ("13. Next steps", Extra::Bullets(4)),
];

const DEFINITIONS: [&str; 7] = [
    "- **Services** means the managed analytics platform and the support described in Schedule A.",
    "- **Charges** means the fees set out in Schedule B, as varied under clause 5.",
    "- **Confidential Information** means information of either party that is marked, or would \
     reasonably be treated, as confidential.",
    "- **Personal Data** has the meaning given in the data processing terms in clause 8.",
    "- **Service Credit** means the amount calculated under paragraph 2 of Schedule B.",
    "- **Change Request** means a written proposal to vary the Services or the Charges under \
     clause 6.",
    "- **Business Day** means a day other than a Saturday, a Sunday or a public holiday in England.",
];

const TABLE: &str = "\
| Item | Clause | Current position | Our recommendation |
| --- | --- | --- | --- |
| Base charges | 5.1 | Fixed for year one, then indexed | Cap the annual rise at the lower of CPI and 3 per cent |
| Service credits | Schedule B, paragraph 2 | Up to 10 per cent of the monthly fees | Raise the ceiling to 15 per cent for critical services |
| Late payment interest | § 5.4 | Eight per cent above base rate | Limit to the statutory rate and exclude disputed sums |
| Expenses | 5.5 | Recoverable on production of receipts | Require prior approval above a set threshold |
| Set-off | § 5.7 | A unilateral right to set off | Limit the right to undisputed sums that are final |
| Invoicing | 5.2 | Monthly in arrears, payable in thirty days | Keep thirty days and add a fifteen day dispute window |";

/// Bullets: one labelled point per topic, each in the topic's own words.
fn bullets(rng: &mut Rng, count: usize) -> String {
    let items: Vec<String> = (0..count)
        .map(|_| {
            let topic = rng.topic();
            format!("- **{}:** {}.", rng.pick(LABELS), capitalise(topic.reason))
        })
        .collect();
    items.join("\n")
}

/// Numbered recommendations, one per topic, distinct within the list.
fn numbered(rng: &mut Rng, count: usize) -> String {
    let start = rng.below(TOPICS.len());
    let items: Vec<String> = (0..count)
        .map(|i| {
            let topic = &TOPICS[(start + i) % TOPICS.len()];
            format!("{}. {}.", i + 1, capitalise(topic.action))
        })
        .collect();
    items.join("\n")
}

/// Three to six sentences on one line.
fn paragraph(rng: &mut Rng) -> String {
    let count = rng.between(3, 6);
    let sentences: Vec<String> = (0..count).map(|_| sentence(rng)).collect();
    sentences.join(" ")
}

fn sentence(rng: &mut Rng) -> String {
    match rng.below(8) {
        0 | 1 => clause_sentence(rng),
        2 => assessment(rng),
        3 => recommendation(rng),
        4 => labelled(rng),
        5 => scenario(rng),
        6 => format!(
            "This point links to {}, which {}.",
            rng.pick(REFERENCES),
            rng.pick(EFFECTS)
        ),
        _ => rng.pick(CONNECTIVES).to_owned(),
    }
}

/// What a clause does. One time in three the clause is in bold.
fn clause_sentence(rng: &mut Rng) -> String {
    let topic = rng.topic();
    if rng.below(3) == 0 {
        format!("**{}** {}.", topic.clause, topic.rule)
    } else {
        format!("{} {}.", topic.clause, topic.rule)
    }
}

fn assessment(rng: &mut Rng) -> String {
    let topic = rng.topic();
    format!(
        "In our view, {} is {}, because {}.",
        topic.short, topic.verdict, topic.reason
    )
}

fn recommendation(rng: &mut Rng) -> String {
    let topic = rng.topic();
    format!(
        "We recommend that the Client {} {}.",
        topic.action,
        rng.pick(ENDINGS)
    )
}

fn labelled(rng: &mut Rng) -> String {
    let topic = rng.topic();
    format!("**{}:** {}.", rng.pick(LABELS), capitalise(topic.reason))
}

fn scenario(rng: &mut Rng) -> String {
    format!(
        "If the Supplier {}, the Client may {}, but only {}.",
        rng.pick(FAILURES),
        rng.pick(REMEDIES),
        rng.pick(LIMITS)
    )
}

/// `text` with its first letter capitalised. The word lists are ASCII.
fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// One matter the memo raises. Its parts agree with one another: the same clause, verdict,
/// reason and action, so no sentence pairs a clause with someone else's reason.
struct Topic {
    /// The clause, with its number, as a subject: "Clause 12.1".
    clause: &'static str,
    /// What the clause does, as a predicate after the clause.
    rule: &'static str,
    /// The matter in lower case, for "In our view, ... is ...".
    short: &'static str,
    verdict: &'static str,
    reason: &'static str,
    /// The recommendation, as a bare verb phrase after "the Client".
    action: &'static str,
}

static TOPICS: [Topic; 14] = [
    Topic {
        clause: "Clause 12.1",
        rule: "limits the liability of each party to the fees paid in the previous twelve months",
        short: "the liability cap in clause 12.1",
        verdict: "unusually narrow for a contract of this value",
        reason: "the cap is set by reference to fees paid rather than fees payable",
        action: "propose a mutual cap that excludes fraud and wilful default",
    },
    Topic {
        clause: "Clause 9.2",
        rule: "gives the Supplier a right to suspend the Services on ten Business Days notice",
        short: "the suspension right in clause 9.2",
        verdict: "a material risk for the Client",
        reason: "the trigger for suspension is not defined",
        action: "ask for a defined trigger, and for suspension to follow a cure period",
    },
    Topic {
        clause: "Clause 6.1",
        rule: "deems each party to have approved every change request it has not rejected in writing",
        short: "the change control procedure in clause 6",
        verdict: "acceptable, provided the drafting is tightened",
        reason: "the deemed approval rule has no time limit",
        action: "limit the deemed approval rule to change requests that state a deadline",
    },
    Topic {
        clause: "Clause 7.4",
        rule: "allows the Supplier to set off sums it considers due against any amount payable",
        short: "the set-off right in clause 7.4",
        verdict: "a material risk for the Client",
        reason: "the right is not limited to sums that are undisputed",
        action: "limit the set-off right to undisputed sums",
    },
    Topic {
        clause: "Clause 8.3",
        rule: "obliges the Supplier to pass Personal Data only to approved subprocessors",
        short: "the data processing terms in clause 8",
        verdict: "silent on the point that matters most",
        reason: "the register of subprocessors is not kept up to date",
        action: "ask for a right to audit subprocessors once in each year",
    },
    Topic {
        clause: "Clause 10.4",
        rule: "requires the Supplier to keep an up to date register of every subcontractor",
        short: "the audit right in clause 10.4",
        verdict: "more favourable to the Supplier than we would expect",
        reason: "the audit right is limited to one visit a year and leaves out subcontractors",
        action: "add an express right to audit subcontractors",
    },
    Topic {
        clause: "Clause 11.1",
        rule: "obliges the Client to give written notice of any change in control within thirty days",
        short: "the change of control clause in clause 11.1",
        verdict: "one-sided in a way that is hard to justify commercially",
        reason: "the notice period runs from the wrong date",
        action: "move the deadline so that it runs from receipt of the notice",
    },
    Topic {
        clause: "Clause 13.3",
        rule: "requires the Supplier to hold professional indemnity insurance of at least five million pounds",
        short: "the insurance requirement in clause 13",
        verdict: "broadly in line with market practice",
        reason: "the cover is not tied to the liability cap in clause 12.1",
        action: "align the insurance cover with the liability cap",
    },
    Topic {
        clause: "Clause 14.2",
        rule: "allows either party to terminate for convenience once the first year has ended",
        short: "the termination right in clause 14.2",
        verdict: "acceptable, provided the drafting is tightened",
        reason: "the notice period in clause 14.2 is shorter than the one in clause 9",
        action: "align the notice periods in clauses 9 and 14",
    },
    Topic {
        clause: "Clause 14.6",
        rule: "requires the Supplier to return all Client data within thirty days of termination",
        short: "the exit assistance obligation in clause 14.6",
        verdict: "unusually narrow for a contract of this value",
        reason: "the exit plan is not described anywhere in the draft",
        action: "add an exit plan to Schedule B with a fixed timetable",
    },
    Topic {
        clause: "Clause 15.5",
        rule: "lets the Supplier rely on force majeure for any delay, however long it lasts",
        short: "the force majeure relief in clause 15.5",
        verdict: "a material risk for the Client",
        reason: "the relief has no time limit and no duty to mitigate",
        action: "cap the relief at ninety days and add a duty to mitigate",
    },
    Topic {
        clause: "Schedule B, paragraph 2",
        rule: "entitles the Client to recover service credits without proof of loss",
        short: "the service credit regime in Schedule B",
        verdict: "broadly in line with market practice",
        reason: "the ceiling of 10 per cent of the monthly fees is low for critical services",
        action: "raise the ceiling to 15 per cent for critical services",
    },
    Topic {
        clause: "Clause 5.6",
        rule: "permits the Supplier to increase the Charges once in each contract year",
        short: "the indexation clause in clause 5.6",
        verdict: "more favourable to the Supplier than we would expect",
        reason: "the index is not named",
        action: "name the index and cap the annual increase",
    },
    Topic {
        clause: "Clause 17.1",
        rule: "obliges the Client to keep the Supplier's pricing confidential",
        short: "the confidentiality clause in clause 17.1",
        verdict: "acceptable, provided the drafting is tightened",
        reason: "it does not say whether the duty survives termination",
        action: "make the duty survive termination for three years only",
    },
];

const THOUGHTS: &[&str] = &[
    "Let me read the master services agreement before I say anything about the liability cap.",
    "The indemnity in clause 11 looks wide, so I will check it against the cap.",
    "Schedule B sets the service credits, and they may not match what clause 9 says.",
    "I need the notice periods for termination, so I will search the matter for them.",
    "The fees table is the part the client will read first, so it has to be exact.",
    "The data processing terms are short, but they carry most of the regulatory risk.",
    "I will search for the defined terms, because the draft uses two of them in two senses.",
    "The edit to clause 4 changes a commercial point, so the client has to approve it first.",
    "Before I draft the memo I want to see whether the audit right covers subprocessors.",
    "The force majeure clause has no time limit, which is unusual for a contract of this size.",
    "I should keep the answer plain and say clearly where the draft is silent.",
    "Nothing here is urgent, but the set-off right should go on the negotiation list.",
];

const LABELS: &[&str] = &[
    "Key risk",
    "Main point",
    "Practical effect",
    "Open question",
    "Drafting gap",
    "Commercial note",
    "Negotiation tip",
];

const ENDINGS: &[&str] = &[
    "before the agreement is signed",
    "in the next round of drafting",
    "before the commercial meeting",
    "as a condition of signature",
];

const REFERENCES: &[&str] = &[
    "clause 6.1",
    "clause 9.3",
    "Schedule B, paragraph 3",
    "the order of precedence in clause 1.4",
    "the definitions in clause 2",
];

const EFFECTS: &[&str] = &[
    "sets the baseline for the same risk",
    "decides which regime applies in practice",
    "a court would read against this clause",
    "needs to be read together with it",
];

const FAILURES: &[&str] = &[
    "misses the same service level in two consecutive months",
    "suspends the Services without the notice that clause 9 requires",
    "fails to restore a critical system within the response time",
    "passes Personal Data outside the approved territory",
];

const REMEDIES: &[&str] = &[
    "claim damages",
    "terminate the affected Service",
    "withhold a proportionate part of the Charges",
    "require a remediation plan within ten Business Days",
];

const LIMITS: &[&str] = &[
    "subject to the cap in clause 12.1",
    "if it gives notice in the form set out in clause 16",
    "once the cure period has expired",
    "for the Services that are actually affected",
];

const CONNECTIVES: &[&str] = &[
    "This is the point we would raise first.",
    "The position is not unusual, but it is not neutral either.",
    "The Client should not accept the drafting in its current form.",
    "Each of these points is capable of a short, practical fix.",
    "Taken together, the clauses shift risk towards the Client.",
    "The commercial team should weigh this against the price.",
    "Nothing in the draft deals with the point expressly.",
    "On balance the risk is manageable with careful negotiation.",
];
