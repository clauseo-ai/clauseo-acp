// The reader's words: the judgment reader, Indian Kanoon's results and its guard, the CCI and NCLAT index, citation
// checks in a draft, and Word's tracked changes. As in words.ts the English is the key, the exact string on screen,
// and a key with slots fills them in either language; {time} takes clock(), so Hindi puts the part of the day first.
// The Hindi uses the formal आप, the danda, Western digits, and English names in Latin script. "powered by IKanoon" is
// Indian Kanoon's own line and stays in English. Judgments' names, dates and words are content, shown as given.
import { clock, t as roomWords, type Key } from "./words.ts";
import { ui, type UiKey } from "./words-ui.ts";
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";

const HI = {
  // ---------- the judgment reader ----------
  "Quoted in the answer": "उत्तर में उद्धृत",
  "Para {n}": "पैरा {n}",
  "Paragraph {n}, quoted in the answer. Scroll back to it": "पैरा {n}, उत्तर में उद्धृत। वापस उस पर जाएँ",
  "{name}, the judgment": "{name}, निर्णय",
  "Cited as": "उद्धरण",
  "SCC first, then AIR": "पहले SCC, फिर AIR",
  "{cite}, at {page}, para {para}": "{cite}, पृष्ठ {page}, पैरा {para}",
  "{cite}, para {para}": "{cite}, पैरा {para}",
  "Copy citation": "उद्धरण कॉपी करें",
  "{n} judgments since {year}": "{year} से {n} निर्णय",
  "1 judgment since {year}": "{year} से 1 निर्णय",
  "Continuing, expected by {time}": "जारी रखने पर {time} बजे तक अपेक्षित",
  Followed: "अनुसरित",
  Distinguished: "विभेदित",
  Doubted: "संदेह व्यक्त",
  "No judgment has doubted it.": "किसी निर्णय ने इस पर संदेह व्यक्त नहीं किया है।",
  "Followed in {a} more and distinguished in {b} more, mostly in the High Courts.":
    "{a} अन्य निर्णयों में अनुसरित और {b} अन्य में विभेदित, अधिकतर उच्च न्यायालयों में।",
  "Checked at {time} against {n} judgments that cite it.":
    "{time} बजे इसे उद्धृत करने वाले {n} निर्णयों से जाँचा गया।",
  "Read from Indian Kanoon at {time}. Kept on this computer, so it opens again without a lookup.":
    "{time} बजे Indian Kanoon से पढ़ा गया। यह इसी कंप्यूटर पर रखा है, इसलिए दोबारा बिना लुकअप के खुलता है।",
  "Kept on this computer. It opens while Indian Kanoon is unavailable.":
    "इसी कंप्यूटर पर रखा है। Indian Kanoon उपलब्ध न होने पर भी यह खुलता है।",

  // ---------- research results ----------
  "Supreme Court": "सर्वोच्च न्यायालय",
  "Results from Indian Kanoon": "Indian Kanoon से परिणाम",
  "High Courts": "उच्च न्यायालय",
  "{n} judgments": "{n} निर्णय",
  "Read {time}": "{time} बजे पढ़ा गया",
  "Not read yet": "अभी पढ़ा नहीं गया",
  "Opens when Indian Kanoon answers": "Indian Kanoon के उत्तर देने पर खुलेगा",
  "Show the {n} High Court judgments": "उच्च न्यायालयों के {n} निर्णय दिखाएँ",
  "Show fewer": "कम दिखाएँ",

  // ---------- the guard ----------
  "This research has used 300 Indian Kanoon lookups. Continue?":
    "इस शोध में Indian Kanoon पर 300 लुकअप हो चुके हैं। जारी रखें?",
  "A lookup is one search or one judgment read. Most research needs fewer than 300, so Clauseo asks before going on.":
    "एक लुकअप यानी एक खोज या एक निर्णय पढ़ना। अधिकतर शोध में 300 से कम लगते हैं, इसलिए आगे बढ़ने से पहले Clauseo पूछता है।",
  "So far {s} searches and {d} judgments read. {f} judgments fit.":
    "अब तक {s} खोजें हुईं और {d} निर्णय पढ़े गए। {f} निर्णय प्रासंगिक हैं।",
  Continue: "जारी रखें",
  "Answer with what's found": "जो मिला उसी से उत्तर दें",
  "Continued past 300 lookups at {time}.": "{time} बजे 300 लुकअप के बाद भी जारी रखा गया।",
  "Stopped at 300 lookups at {time}. Clauseo answered with what was found.":
    "{time} बजे 300 लुकअप पर रोका गया। Clauseo ने जो मिला उसी से उत्तर दिया।",

  // ---------- Indian Kanoon unavailable ----------
  "Indian Kanoon is unavailable": "Indian Kanoon उपलब्ध नहीं है",
  "Searches wait until it answers. Clauseo tries again at {time}.":
    "उसके उत्तर देने तक खोजें प्रतीक्षा करेंगी। Clauseo {time} बजे फिर प्रयास करेगा।",
  "Judgments already read still open.": "पहले पढ़े गए निर्णय अब भी खुलते हैं।",

  // ---------- the CCI and NCLAT index ----------
  "CCI and NCLAT index": "CCI और NCLAT सूचकांक",
  "Snapshot of {date}": "{date} का स्नैपशॉट",
  "An index, not a record. Read the order itself before relying on it.":
    "यह सूचकांक है, अभिलेख नहीं। इस पर भरोसा करने से पहले आदेश स्वयं पढ़ें।",
  Case: "प्रकरण",
  Parties: "पक्षकार",
  Under: "धारा",
  Order: "आदेश",
  Outcome: "परिणाम",
  Appeal: "अपील",
  "About this index": "इस सूचकांक के बारे में",
  "About {cases} CCI cases and {appeals} NCLAT appeals, kept on this computer.":
    "CCI के लगभग {cases} मामले और NCLAT की {appeals} अपीलें, इसी कंप्यूटर पर रखी हैं।",
  "The snapshot comes with Clauseo's updates. Its date says how current it is.":
    "स्नैपशॉट Clauseo के अपडेट के साथ आता है। उसकी तारीख बताती है कि वह कितना ताज़ा है।",
  "Open the order on cci.gov.in": "cci.gov.in पर आदेश खोलें",
  "Index entry": "सूचकांक प्रविष्टि",

  // ---------- the draft in review: Word's tracked changes, and its citations checked ----------
  "Open in Word": "Word में खोलें",
  "Open in Word since {time}": "{time} बजे से Word में खुला",
  Changes: "बदलाव",
  Citations: "उद्धरण",
  "Changes and citations": "बदलाव और उद्धरण",
  Accept: "स्वीकार करें",
  Reject: "अस्वीकार करें",
  "Accept all": "सभी स्वीकार करें",
  "Reject all": "सभी अस्वीकार करें",
  "Not yet accepted": "अभी स्वीकृत नहीं",
  "Accepted {time}": "{time} बजे स्वीकृत",
  "Rejected {time}": "{time} बजे अस्वीकृत",
  "You, in Word": "आप, Word में",
  "Today, {time}": "आज, {time}",
  Added: "जोड़ा गया",
  "Taken out": "हटाया गया",
  "{n} changes, each with its author and time, as Word shows them.":
    "{n} बदलाव, हर एक अपने लेखक और समय के साथ, जैसे Word उन्हें दिखाता है।",
  "Accepting here accepts the change in the Word file too.":
    "यहाँ स्वीकार करने से बदलाव Word फ़ाइल में भी स्वीकार हो जाता है।",
  "Your edits in Word came back at {time}.": "Word में किए गए आपके संपादन {time} बजे वापस आ गए।",
  "{n} changes, by you.": "{n} बदलाव, आपके द्वारा।",
  "Close '{file}' in Word to accept these changes.":
    "ये बदलाव स्वीकार करने के लिए Word में '{file}' बंद करें।",
  "Close '{file}' in Word to reject these changes.":
    "ये बदलाव अस्वीकार करने के लिए Word में '{file}' बंद करें।",
  "Word holds the file while it is open. Nothing has changed yet.":
    "खुली रहने तक फ़ाइल Word के पास रहती है। अभी कुछ नहीं बदला है।",
  "Try again": "फिर से प्रयास करें",
  "Change by {who}, {when}": "{who} का बदलाव, {when}",

  // the citations checked
  "Checked {n} citations at {time}.": "{time} बजे {n} उद्धरण जाँचे गए।",
  "None of these checks holds back saving or accepting.":
    "इनमें से कोई जाँच सहेजने या स्वीकार करने से नहीं रोकती।",
  Verified: "सत्यापित",
  Unverifiable: "सत्यापन संभव नहीं",
  Mismatched: "मेल नहीं खाता",
  "Not found": "नहीं मिला",
  Unchecked: "जाँचा नहीं गया",
  Checking: "जाँच जारी",
  "Found at this citation, and good law.": "इसी उद्धरण पर मिला, और अब भी मान्य है।",
  "The quote in the draft differs from the judgment's own words.":
    "प्रारूप का उद्धरण निर्णय के अपने शब्दों से भिन्न है।",
  "The draft says": "प्रारूप में है",
  "Paragraph {n} says": "पैरा {n} में है",
  "Indian Kanoon has no judgment at {cite}. {name} is reported at {right}.":
    "Indian Kanoon पर {cite} पर कोई निर्णय नहीं है। {name} {right} पर प्रकाशित है।",
  "This order isn't on Indian Kanoon, so its words can't be checked here.":
    "यह आदेश Indian Kanoon पर नहीं है, इसलिए यहाँ इसके शब्द जाँचे नहीं जा सकते।",
  "Added in Word at {time}, after the last check.": "अंतिम जाँच के बाद, {time} बजे Word में जोड़ा गया।",
  "Check again": "फिर से जाँचें",
  "Reading the judgment": "निर्णय पढ़ा जा रहा है",
  "In Clauseo's draft": "Clauseo के प्रारूप में",
  "In your edit in Word": "Word में आपके संपादन में",
} as const;

/** Every string of the reader, in English. */
export type ReaderKey = keyof typeof HI;

/** A string of the reader in the language, its slots filled. */
export const rdt = wordsOf<ReaderKey>(HI);

/** Every Hindi string here, for the tests' banned-words screen. */
export const readerHindi: readonly string[] = Object.values(HI);
/** Every English key here, for the same screen. */
export const readerEnglish: readonly string[] = Object.keys(HI);

/** The reader's words in the display's language, bound, with the room's and the interface's beside them. */
export function useReaderWords() {
  const { lang } = useDisplay();
  return {
    lang,
    r: (key: ReaderKey, slots?: Slots) => rdt(lang, key, slots),
    t: (key: Key, slots?: Slots) => roomWords(lang, key, slots),
    ui: (key: UiKey, slots?: Slots) => ui(lang, key, slots),
    clock: (time: string) => clock(lang, time),
  };
}
