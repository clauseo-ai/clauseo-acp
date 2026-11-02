// The words of the live thread: the working line, errors, the permission prompt, the change note, the composer and
// the empty states. As in words.ts the English is the key, the exact string on screen, and a key with slots fills
// them in either language.
// The Hindi is a specialist's draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits,
// English legal names in Latin script. Clauseo never says मैं, and nothing says पेंसिल, स्याही, चरण or कदम.
import { clock, t as roomWords, type Key } from "./words.ts";
import { ui, type UiKey } from "./words-ui.ts";
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";

const HI = {
  // the working line: one check, then a series, naming the judgment being checked
  "Checking {name}. Reading {waitingFor}.": "{name} की जाँच हो रही है। {waitingFor} पढ़े जा रहे हैं।",
  "Checking {n} judgments against later decisions": "{n} निर्णयों का बाद के निर्णयों से मिलान हो रहा है",
  "Now {name}.": "अब {name}।",
  // waiting for another answer in the same matter
  "Clauseo is still answering in this matter. Your question waits until then.":
    "Clauseo इस मामले में अभी उत्तर दे रहा है। तब तक आपका प्रश्न प्रतीक्षा में है।",
  // errors, in concrete, with what the lawyer wrote kept
  "Asking at {time}": "{time} बजे पूछा जाएगा",
  "It resets at {time}. Your question is saved.": "यह {time} बजे रीसेट होगा। आपका प्रश्न सहेजा गया है।",
  "Claude is not responding.": "Claude से उत्तर नहीं आ रहा है।",
  "Retrying at {time}": "{time} बजे फिर प्रयास होगा",
  "Retry now": "अभी फिर से प्रयास करें",
  Details: "विवरण",
  "Claude last replied at {time}. Your question and the answer so far are kept.":
    "Claude ने आखिरी बार {time} बजे जवाब दिया। आपका प्रश्न और अब तक का उत्तर सुरक्षित हैं।",
  "Clauseo stopped. Your question and the answer so far are kept.":
    "Clauseo रुक गया। आपका प्रश्न और अब तक का उत्तर सुरक्षित हैं।",
  Restart: "फिर से शुरू करें",
  // the lawyer stopping the answer
  "Stop the answer": "उत्तर रोकें",
  // how a turn ended, one line in the thread; a tool call that failed or was stopped
  Failed: "विफल",
  Stopped: "रोका गया",
  "You stopped the answer. The answer so far is kept.":
    "आपने उत्तर रोक दिया। अब तक का उत्तर रखा गया है।",
  "The answer stopped at its limit, before it finished.": "उत्तर पूरा होने से पहले अपनी सीमा पर रुक गया।",
  "This question was declined, so there is no answer.":
    "इस प्रश्न को अस्वीकार किया गया, इसलिए कोई उत्तर नहीं है।",
  "The answer stopped. Your question and the answer so far are kept.":
    "उत्तर रुक गया। आपका प्रश्न और अब तक का उत्तर रखे गए हैं।",
  // who said each row, for assistive technology ("You" is words.ts's)
  "Your question": "आपका प्रश्न",
  "Clauseo's answer": "Clauseo का उत्तर",
  // the same, with the clock time each began (the question's) or ended (the answer's step), so every exchange's
  // sections have names of their own; the plain ones above where no time is known yet
  "Your question, {time}": "आपका प्रश्न, {time}",
  "Clauseo's answer, {time}": "Clauseo का उत्तर, {time}",
  // content the thread can't show: a named attachment, or anything else
  "Attached: {name}": "संलग्न: {name}",
  "Something here can't be shown in the thread.": "यहाँ का कुछ हिस्सा बातचीत में नहीं दिखाया जा सकता।",
  // the work behind an answer
  // the permission prompt, inline in the thread
  "Clauseo wants to save a new version of {paper}.": "Clauseo {paper} का नया संस्करण सहेजना चाहता है।",
  "Allow once": "एक बार अनुमति दें",
  "Always for this matter": "इस मामले के लिए हमेशा",
  "Don't allow": "अनुमति न दें",
  // drafts for Rohan (U4): the fourth answer, the ask line when the agent names no paper, and the folded lines when
  // nothing was saved or the question is gone; each first sentence matches the folded lines below
  "Don't allow for this matter": "इस मामले के लिए अनुमति न दें",
  "Clauseo asks before going on.": "आगे बढ़ने से पहले Clauseo आपसे पूछता है।",
  "No longer waiting for an answer.": "अब उत्तर की प्रतीक्षा नहीं है।",
  "Allowed once.": "एक बार अनुमति दी गई।",
  "Allowed for this matter.": "इस मामले के लिए अनुमति दी गई।",
  "Not allowed.": "अनुमति नहीं दी गई।",
  "Not allowed for this matter.": "इस मामले के लिए अनुमति नहीं दी गई।",
  // what the answered prompt says: what the lawyer decided first, then what Clauseo did, in the past tense
  "Allowed once. Clauseo saved a new version of {paper}.":
    "एक बार अनुमति दी गई। Clauseo ने {paper} का नया संस्करण सहेजा।",
  "Allowed for this matter. Clauseo saved a new version of {paper}.":
    "इस मामले के लिए अनुमति दी गई। Clauseo ने {paper} का नया संस्करण सहेजा।",
  "Not allowed. Clauseo left {paper} as it was.":
    "अनुमति नहीं दी गई। Clauseo ने {paper} में कोई बदलाव नहीं किया।",
  // drafts for Rohan (U4b): the agent's question as a form, inline in the thread. "Other" leads a question's own
  // free-text answer; "Required" marks a question that must be answered; the line under the answers shows while
  // nothing is filled in, since an answer with nothing in it is the same as Don't answer. "Don't answer" matches the
  // permission prompt's "Don't allow"
  Send: "भेजें",
  "Don't answer": "उत्तर न दें",
  Other: "अन्य",
  Required: "आवश्यक",
  "Answer this question.": "इस प्रश्न का उत्तर दें।",
  // a field that doesn't fit and whose schema names no limit; one that does names it below
  "This answer doesn't fit the question.": "यह उत्तर प्रश्न से मेल नहीं खाता।",
  "Choose an answer to send, or 'Don't answer' to go on without one.":
    "भेजने के लिए कोई उत्तर चुनें, या बिना उत्तर के आगे बढ़ने के लिए 'उत्तर न दें' चुनें।",
  // a question whose form Clauseo can't draw: the two ways on
  "This question can't be shown here. Choose 'Don't answer', or Stop the answer.":
    "यह प्रश्न यहाँ नहीं दिखाया जा सकता। 'उत्तर न दें' चुनें, या उत्तर रोकें।",
  // the limit a field's answer broke, as the agent's schema gives it
  "Use {n} characters or fewer.": "{n} या उससे कम अक्षर लिखें।",
  "Use 1 character or fewer.": "1 या उससे कम अक्षर लिखें।",
  "Use at least {n} characters.": "कम से कम {n} अक्षर लिखें।",
  "Choose up to {n}.": "अधिकतम {n} चुनें।",
  "Choose at least {n}.": "कम से कम {n} चुनें।",
  "Use a number from {min} to {max}.": "{min} से {max} तक की कोई संख्या लिखें।",
  "Use a number of {min} or more.": "{min} या उससे अधिक की संख्या लिखें।",
  "Use a number of {max} or less.": "{max} या उससे कम की संख्या लिखें।",
  "Use a whole number.": "पूर्ण संख्या लिखें।",
  // the change note
  "Clauseo changed {n} papers.": "Clauseo ने {n} दस्तावेज़ बदले।",
  "Clauseo changed 1 paper.": "Clauseo ने 1 दस्तावेज़ बदला।",
  Compare: "तुलना करें",
  "{name} changed after Clauseo's edit.": "Clauseo के संपादन के बाद {name} में बदलाव हुए हैं।",
  "You undid Clauseo's changes to {n} papers.": "आपने {n} दस्तावेज़ों में Clauseo के बदलाव पूर्ववत किए।",
  "You undid Clauseo's changes to 1 paper.": "आपने 1 दस्तावेज़ में Clauseo के बदलाव पूर्ववत किए।",
  // the composer
  "Papers for this question": "इस प्रश्न के दस्तावेज़",
  "Remove {title}": "{title} हटाएँ",
  // the composer's `/` menu, the agent's commands, named for assistive technology
  Commands: "कमांड",
  // and said politely as the menu opens, for a screen reader
  "1 command": "1 कमांड",
  "{n} commands": "{n} कमांड",
  // the composer's `@` menu, the matter's papers: its name for assistive technology, the count said as it opens, and
  // the lines under its rows (loading, a list that couldn't be read, more matches than rows, a list that stops short;
  // a matter with none says words.ts's "No papers yet.")
  Papers: "दस्तावेज़",
  "1 paper": "1 दस्तावेज़",
  "{n} papers": "{n} दस्तावेज़",
  "Loading papers…": "दस्तावेज़ लोड हो रहे हैं…",
  "Couldn't read this matter's papers.": "इस मामले के दस्तावेज़ पढ़े नहीं जा सके।",
  "First {shown} of {n}. Type to narrow.": "{n} में से पहले {shown}। सूची छोटी करने के लिए आगे टाइप करें।",
  "Not every file in this folder is listed.": "इस फ़ोल्डर की सभी फ़ाइलें सूची में नहीं हैं।",
  // the plan in the status line, for a plan other than Claude
  "Uses your {name} plan": "आपके {name} प्लान का उपयोग करता है",
  // empty states
  "Clauseo has read this matter's {n} papers.": "Clauseo ने इस मामले के {n} दस्तावेज़ पढ़ लिए हैं।",
  "Ask the first question below. Each answer cites the paper and page it relies on.":
    "नीचे पहला प्रश्न पूछें। हर उत्तर उसी दस्तावेज़ और पृष्ठ का हवाला देता है जिस पर वह आधारित है।",
  "Nothing in this matter matches '{q}'.": "इस मामले में '{q}' से मेल खाता कुछ नहीं है।",
  "Search covers the papers, the answers and Versions.": "खोज में दस्तावेज़, उत्तर और संस्करण शामिल हैं।",
} as const;

/** Every new string of the live thread, in English. */
export type ThreadKey = keyof typeof HI;

/** A new string in the language, its slots filled. */
export const th = wordsOf<ThreadKey>(HI);

/** The thread's words of the page's language, bound, with the shared ones beside them:
 * const w = useThreadWords(); w.th("Allow once"), w.t("Undo"), w.ui("Undo anyway"). */
export function useThreadWords() {
  const { lang } = useDisplay();
  return {
    lang,
    th: (key: ThreadKey, slots?: Slots) => th(lang, key, slots),
    t: (key: Key, slots?: Slots) => roomWords(lang, key, slots),
    ui: (key: UiKey, slots?: Slots) => ui(lang, key, slots),
    clock: (time: string) => clock(lang, time),
  };
}
