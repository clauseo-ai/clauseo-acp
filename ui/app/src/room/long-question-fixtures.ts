// Scratch fixtures for the long-question audit (probe-main.tsx's ?question=2page|50page|2page-hi): a very long pasted
// question in fake legal text, between fake parties, and a short answer. The text is built, not stored: numbered
// paragraphs of sentences drawn in turn from a fixed bank, so every run gives the same words. Not shipped.
// On the probe page, with ?question=4line, 2page, 50page, 2page-hi or 6mib, the thread is one finished exchange whose
// question is a pasted text in fake legal words: about four lines, about 1,000 words, about 27,000 words (160 KB), two
// pages of Hindi, or 6 MiB played as the host sends it (an Upsert with its first 4 KiB, then AppendText patches of
// 4 KiB); window.__clauseoLongOpen times its arrival (delivered, first frame, whole), window.__clauseoLongText(words)
// builds a text to paste into the composer, and window.__clauseoProbeCalls is what was sent.

const PARTIES = [
  "Meridian Agro Traders Pvt. Ltd.",
  "Kestrel Finance Ltd.",
  "Northgate Warehousing LLP",
  "the Applicant",
  "the Respondent",
];

const SENTENCES = [
  "By an agreement dated 4 April 2019, {a} agreed to supply bagged fertiliser to {b} on thirty days' credit.",
  "The agreement provided that any dispute would first be referred to the managing directors of both parties for a period of sixty days.",
  "Clause 14.2 required written notice of any breach, sent by registered post to the address stated in the schedule.",
  "On 18 November 2020, {b} issued a cheque for an amount said to be the balance due, which was returned unpaid with the remark 'funds insufficient'.",
  "{a} sent a demand notice within the statutory period, and {b} replied denying liability and alleging short delivery.",
  "The reply stated that four consignments were received damaged and that a credit note had been promised but never issued.",
  "There is no record on our file of any credit note, and the delivery challans bear the stamp of {c} without remarks.",
  "We need to know whether the reply raises a probable defence that would shift the burden back to the complainant at trial.",
  "Please also consider whether the dispute resolution clause bars the complaint, or whether it applies only to civil claims.",
  "The managing director of {a} is willing to give evidence, but the warehouse manager who signed the challans has since left.",
  "Our client wants to know the likely timeline, the costs exposure, and whether a summary suit is worth filing in parallel.",
  "In the reply, {b} also claims that the cheque was given as security and not towards a discharge of any existing debt.",
  "The ledger extract shows a running account, with part payments made in March and June 2020 against older invoices.",
  "Please set out the points a magistrate is likely to treat as decisive, and the documents we should collect before the first hearing.",
  "If the court finds the notice defective, can a fresh notice be sent on a re-presentation of the same cheque?",
  "We would also like a short view on interest: the invoices state eighteen per cent a year, but the agreement is silent.",
];

/** One paragraph: its number, then `n` sentences from the bank, starting at `at`, with the parties filled in. */
function paragraph(no: number, at: number, n: number): string {
  const out: string[] = [];
  for (let i = 0; i < n; i++) {
    const s = SENTENCES[(at + i) % SENTENCES.length]!;
    out.push(
      s
        .replaceAll("{a}", PARTIES[0]!)
        .replaceAll("{b}", PARTIES[1]!)
        .replaceAll("{c}", PARTIES[2]!),
    );
  }
  return `${no}. ${out.join(" ")}`;
}

/** A question of about `words` words, in numbered paragraphs of four to seven sentences, blank lines between. */
export function longQuestion(words: number): string {
  const parts = [
    "Please advise on the following facts in Meridian Agro Traders Pvt. Ltd. v. Kestrel Finance Ltd., a complaint under section 138 of the Negotiable Instruments Act.",
  ];
  let count = parts[0]!.split(/\s+/).length;
  let at = 0;
  for (let no = 1; count < words; no++) {
    const n = 4 + (no % 4);
    const p = paragraph(no, at, n);
    at += n;
    parts.push(p);
    count += p.split(/\s+/).length;
  }
  return parts.join("\n\n");
}

const HINDI_SENTENCES = [
  "दिनांक 4 अप्रैल 2019 के अनुबंध द्वारा मेरिडियन एग्रो ट्रेडर्स प्रा. लि. ने केस्ट्रेल फ़ाइनेंस लि. को तीस दिन के उधार पर खाद की आपूर्ति करना स्वीकार किया।",
  "अनुबंध की धारा 14.2 के अनुसार किसी भी उल्लंघन की लिखित सूचना पंजीकृत डाक से अनुसूची में दिए पते पर भेजी जानी थी।",
  "18 नवंबर 2020 को प्रत्यर्थी द्वारा जारी चेक 'अपर्याप्त निधि' की टिप्पणी के साथ बिना भुगतान लौटा दिया गया।",
  "मांग सूचना वैधानिक अवधि में भेजी गई, और प्रत्यर्थी ने दायित्व से इनकार करते हुए कम आपूर्ति का आरोप लगाया।",
  "कृपया बताएँ कि क्या उत्तर में ऐसा संभावित बचाव है जिससे विचारण में भार फिर से परिवादी पर आ जाए।",
  "हमारे मुवक्किल जानना चाहते हैं कि संभावित समय-सीमा क्या होगी, खर्च कितना होगा, और क्या साथ में संक्षिप्त वाद दायर करना उचित है।",
];

/** The Hindi question, about two pages: numbered paragraphs of the Hindi bank. */
export function longHindiQuestion(): string {
  const parts: string[] = [];
  for (let no = 1; no <= 14; no++) {
    const out: string[] = [];
    for (let i = 0; i < 5; i++) out.push(HINDI_SENTENCES[(no + i) % HINDI_SENTENCES.length]!);
    parts.push(`${no}. ${out.join(" ")}`);
  }
  return parts.join("\n\n");
}

export const LONG_ANSWER = [
  "**Short answer.** The reply's defences are arguable but thin on the papers you describe. The presumption that the cheque was for a debt stands until Kestrel Finance Ltd. shows a probable defence, and an unproved credit note is unlikely to be enough on its own.",
  "**The dispute clause.** A clause sending disputes to the managing directors first is a step before civil proceedings. It does not bar a criminal complaint for a dishonoured cheque, which is a statutory offence and not a claim under the agreement.",
  "**Security cheque.** A cheque given as security can still found a complaint if a debt was due on the date it was presented. The running account and the part payments help here: they show a balance outstanding when the cheque was deposited.",
  "**Next steps.** Collect the original challans, the ledger, the postal receipts for the demand notice, and an affidavit from the managing director. Trace the former warehouse manager now: a summons later is slower than a statement today.",
].join("\n\n");

export type LongQuestionKind = "4line" | "2page" | "50page" | "2page-hi" | "6mib";

/** ?question=…: the kind asked for, or null. */
export function longQuestionOf(value: string | null): LongQuestionKind | null {
  return value === "4line" ||
    value === "2page" ||
    value === "50page" ||
    value === "2page-hi" ||
    value === "6mib"
    ? value
    : null;
}

/** 6 MiB, the raised send cap: the host sends such a question as one Upsert holding its first 4 KiB, then AppendText
 * patches of 4 KiB (crates/protocol's INLINE_LIMIT_BYTES), every time the conversation opens. */
export const SIX_MIB = 6 * 1024 * 1024;
export const CHUNK = 4 * 1024;

export function longQuestionText(kind: LongQuestionKind): string {
  if (kind === "2page-hi") return longHindiQuestion();
  // about four lines at 1280: one sentence and a short second paragraph
  if (kind === "4line")
    return `${longQuestion(1)} Please reply by Friday with a short view on the notice and on the next steps.`;
  // all ASCII, so a byte is a UTF-16 code unit and the cut lands on a character
  if (kind === "6mib") return longQuestion(1_080_000).slice(0, SIX_MIB);
  return longQuestion(kind === "2page" ? 1_000 : 27_000);
}
