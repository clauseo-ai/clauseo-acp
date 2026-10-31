// The interface's words. English is the key, the exact string on screen, and Hindi is the value beside it. A key
// with slots, such as "{n} more", fills them in either language. Parts hold no strings of their own: each takes its
// words from here or from the words file of its own area (words-ui.ts, words-frame.ts). The Hindi is a specialist's
// draft, waiting for a Hindi-speaking lawyer's review: the formal आप, the danda, Western digits, and English names in
// Latin script.

import { useDisplay, type Lang } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";

const HI = {
  Matters: "मामले",
  "Next date": "अगली तारीख",
  "Search this matter": "इस मामले में खोजें",
  Undo: "पूर्ववत करें",
  "Versions and Undo": "संस्करण और पूर्ववत करें",
  "New matter": "नया मामला",
  "This matter's thread": "इस मामले की बातचीत",
  "Turn to it": "इस पर जाएँ",
  "Ask about this matter": "इस मामले के बारे में पूछें",
  "Add papers": "दस्तावेज़ जोड़ें",
  Ask: "पूछें",
  Papers: "दस्तावेज़",
  "By paper book page": "पेपर बुक पृष्ठ के अनुसार",
  "All {n}": "सभी {n}",
  Citations: "उद्धृत निर्णय",
  "Mark size follows the bench": "चिह्न का आकार न्यायपीठ के आकार के अनुसार है",
  "Copy list": "सूची कॉपी करें",
  "None cited yet.": "अभी कोई निर्णय उद्धृत नहीं।",
  "{n} more": "{n} और",
  "Scratch matter": "कच्चा काम",
  "Quick questions": "त्वरित प्रश्न",
  "{name} plan": "{name} प्लान",
  "{time} am": "सुबह {time}",
  "{time} pm": "शाम {time}",
  "{time} pm, afternoon": "दोपहर {time}",
  "{time} pm, night": "रात {time}",
  "Accepted {time}": "{time} बजे स्वीकृत",
  "Expected by {time}": "{time} बजे तक अपेक्षित",
  "PB {n}": "PB {n}",
  "PB {from} to {to}": "PB {from} से {to}",
  "to {n}": "से {n}",
  Dates: "तारीखें",
  "List of dates": "तारीखों की सूची",
  Draft: "प्रारूप",
  "{title}, {pb}": "{title}, {pb}",
  "{title}. {accepted}": "{title}। {accepted}",
  Supports: "समर्थन में",
  Against: "विरुद्ध",
  "Relied on": "अवलंबित",
  Prepared: "तैयार",
  Accepted: "स्वीकृत",
  "They will cite": "वे हवाला देंगे",
  "For us": "हमारे पक्ष में",
  "Good law": "अब भी मान्य",
  Checking: "जाँच जारी",
  "Verbatim from the paper": "दस्तावेज़ से शब्दशः",
  Open: "खोलें",
  "Open {title} at {pin}": "{title}, {pin} पर खोलें",
  "Open {name}": "{name} खोलें",
  "{court}, {bench}": "{court}, {bench}",
  "Decided {date}": "निर्णय दिनांक {date}",
  "Read {waitingFor}": "{waitingFor} पढ़े गए",
  "Constitution bench": "संविधान पीठ",
  "Three judges": "तीन न्यायाधीश",
  "Two judges": "दो न्यायाधीश",
  "Single judge": "एकल न्यायाधीश",
  Tribunal: "अधिकरण",
  "{bench}. Mark size follows the bench.": "{bench}। चिह्न का आकार न्यायपीठ के आकार के अनुसार है।",
  "{name}, {cite}": "{name}, {cite}",
  "The list of dates, made earlier": "तारीखों की सूची, पहले बनाई गई",
  You: "आप",
  Clauseo: "Clauseo",
  "Your question: {q} Scroll back to it": "आपका प्रश्न: {q} उस पर वापस जाएँ",
  "This answer": "यह उत्तर",
  "Accept into the matter": "स्वीकार कर मामले में जोड़ें",
  "Not yet accepted": "अभी स्वीकृत नहीं",
  "Undo accepting the answer": "उत्तर की स्वीकृति पूर्ववत करें",
  "{why}.": "{why}।",
  "{title}. {where}.": "{title}। {where}।",
  "{n} pages": "{n} पृष्ठ",
  Opened: "खोला गया",
  "Turn to it: {title}, {where}": "इस पर जाएँ: {title}, {where}",
  "Back to the answer": "उत्तर पर लौटें",
  "Page 1 of {n}": "{n} में से पृष्ठ 1",
  "{title}, page 1 of {n}": "{title}, {n} में से पृष्ठ 1",
  Versus: "बनाम",
  "Page 1": "पृष्ठ 1",
  "Every change is kept.": "हर बदलाव सुरक्षित रहता है।",
  "Claude plan limit reached": "Claude प्लान की सीमा पूरी हो गई",
  "No papers yet.": "अभी कोई दस्तावेज़ नहीं।",
  "Add the matter's papers and Clauseo reads them before your first question.":
    "मामले के दस्तावेज़ जोड़ें। आपके पहले प्रश्न से पहले Clauseo उन्हें पढ़ लेता है।",
  Stop: "रोकें",
} as const;

/** Every interface string this file has in both languages. */
export type Key = keyof typeof HI;

/** A string in the language, its slots filled. */
export const t = wordsOf<Key>(HI);

/** "4:31 pm" in the language: Hindi puts the part of the day first, "शाम 4:31", "दोपहर 3:58". */
export function clock(lang: Lang, time: string): string {
  if (lang === "en") return time;
  return time.replace(/(\d{1,2}:\d{2})\s*(am|pm)/i, (_m: string, tm: string, ap: string) => {
    if (ap.toLowerCase() === "am") return t(lang, "{time} am", { time: tm });
    const h = Number.parseInt(tm, 10) % 12;
    const key: Key = h < 4 ? "{time} pm, afternoon" : h < 7 ? "{time} pm" : "{time} pm, night";
    return t(lang, key, { time: tm });
  });
}

/** The words of the page's language, bound: const w = useWords(); w.t("{n} more", { n: 3 }). */
export function useWords() {
  const { lang } = useDisplay();
  return {
    lang,
    t: (key: Key, slots?: Slots) => t(lang, key, slots),
    clock: (time: string) => clock(lang, time),
  };
}
