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
  // the work behind an answer
  // the permission prompt, inline in the thread
  "Clauseo wants to save a new version of {paper}.": "Clauseo {paper} का नया संस्करण सहेजना चाहता है।",
  "Allow once": "एक बार अनुमति दें",
  "Always for this matter": "इस मामले के लिए हमेशा",
  "Don't allow": "अनुमति न दें",
  // what the answered prompt says: what the lawyer decided first, then what Clauseo did, in the past tense
  "Allowed once. Clauseo saved a new version of {paper}.":
    "एक बार अनुमति दी गई। Clauseo ने {paper} का नया संस्करण सहेजा।",
  "Allowed for this matter. Clauseo saved a new version of {paper}.":
    "इस मामले के लिए अनुमति दी गई। Clauseo ने {paper} का नया संस्करण सहेजा।",
  "Not allowed. Clauseo left {paper} as it was.":
    "अनुमति नहीं दी गई। Clauseo ने {paper} में कोई बदलाव नहीं किया।",
  // the change note
  "Clauseo changed {n} papers.": "Clauseo ने {n} दस्तावेज़ बदले।",
  Compare: "तुलना करें",
  "{name} changed after Clauseo's edit.": "Clauseo के संपादन के बाद {name} में बदलाव हुए हैं।",
  "You undid Clauseo's changes to {n} papers.": "आपने {n} दस्तावेज़ों में Clauseo के बदलाव पूर्ववत किए।",
  // the composer
  "Papers for this question": "इस प्रश्न के दस्तावेज़",
  "Remove {title}": "{title} हटाएँ",
  // the plan in the status line, for a plan other than Claude
  // empty states
  "Clauseo has read this matter's {n} papers.": "Clauseo ने इस मामले के {n} दस्तावेज़ पढ़ लिए हैं।",
  "Ask the first question below. Each answer cites the paper and page it relies on.":
    "नीचे पहला प्रश्न पूछें। हर उत्तर उसी दस्तावेज़ और पृष्ठ का हवाला देता है जिस पर वह आधारित है।",
  "Nothing in this matter matches '{q}'.": "इस मामले में '{q}' से मेल खाता कुछ नहीं है।",
  "Search covers the papers, the answers and History.": "खोज में दस्तावेज़, उत्तर और इतिहास शामिल हैं।",
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
