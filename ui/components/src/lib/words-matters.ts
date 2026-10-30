// The words of living in matters: finding a matter on the rail, a folder that moved, the conversations inside a
// matter, and messages sent while Clauseo works. As in words.ts the English is the key, the exact string on screen,
// and a key with slots fills them in either language; {time} takes clock(), so Hindi puts the part of the day first.
// The Hindi uses the formal आप, the danda, Western digits and English names in Latin script. Nothing here says I or
// मैं, names a model, or says pencil, ink, step, पेंसिल, स्याही, चरण or कदम.
import { clock, t as roomWords, type Key } from "./words.ts";
import { ui, type UiKey } from "./words-ui.ts";
import { frame, type FrameKey } from "./words-frame.ts";
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";

const HI = {
  /* ---------- the rail: finding a matter, a folder that moved ---------- */
  "Find a matter": "मामला खोजें",
  "Find a matter by its name, client or case number": "नाम, मुवक्किल या केस नंबर से मामला खोजें",
  Recent: "हाल के",
  "Matching matters": "मेल खाते मामले",
  "No matter matches '{q}'.": "कोई मामला '{q}' से मेल नहीं खाता।",
  "Search covers names, clients and case numbers.": "खोज में नाम, मुवक्किल और केस नंबर शामिल हैं।",
  "Client, {client}": "मुवक्किल, {client}",
  "Folder moved": "फ़ोल्डर कहीं और ले जाया गया",
  "A matter waits for you": "एक मामला आपकी प्रतीक्षा में है",
  /* ---------- conversations inside a matter ---------- */
  "New conversation": "नई बातचीत",
  "Conversations in {name}": "{name} की बातचीत",
  "Actions for {title}": "{title} के विकल्प",
  "Continue with {agent}": "{agent} के साथ जारी रखें",
  "A new conversation, with a summary of this one and a link back.":
    "एक नई बातचीत, इस बातचीत के सारांश और इस पर लौटने की कड़ी के साथ।",
  "Waiting for '{title}'": "'{title}' की प्रतीक्षा में",
  /* ---------- messages while Clauseo works: the queue ---------- */
  Queue: "कतार में रखें",
  "Queued messages": "कतार के संदेश",
  "{n} queued, sent in order.": "{n} कतार में, क्रम से भेजे जाएँगे।",
  "1 queued, sent once this answer is done.": "1 कतार में, यह उत्तर पूरा होते ही जाएगा।",
  "The queue holds {n}.": "कतार में {n} संदेश रह सकते हैं।",
  "{n} more": "{n} और",
  "Show fewer": "कम दिखाएँ",
  Edit: "बदलें",
  Remove: "हटाएँ",
  "Send now": "अभी भेजें",
  Save: "सहेजें",
  "Edit queued message {n}": "कतार का संदेश {n} बदलें",
  "The queue is held. You stopped the answer at {time}, so nothing more is sent until you send.":
    "कतार रुकी है। आपने {time} बजे उत्तर रोका, इसलिए जब तक आप न भेजें, आगे कुछ नहीं भेजा जाएगा।",
  "The queue is held. The answer failed at {time}, so nothing more is sent until you send.":
    "कतार रुकी है। {time} बजे उत्तर पूरा नहीं हो सका, इसलिए जब तक आप न भेजें, आगे कुछ नहीं भेजा जाएगा।",
  "Send next": "अगला भेजें",
  /* ---------- steering the running answer: the words a host gives SteerHint, SteerMark and SteerNote ---------- */
  "Send now gives your words to Clauseo while it writes.":
    "अभी भेजें से Clauseo लिखते समय ही आपके शब्द ले लेता है।",
  "You, {time}, while Clauseo answered": "आप, {time}, जब Clauseo उत्तर दे रहा था",
  "Your words at {time} came in while Clauseo searched Indian Kanoon for {what}. That search carried on in the background, finished at {done}, and this answer uses it.":
    "आपके {time} बजे के शब्द तब पहुँचे जब Clauseo Indian Kanoon पर {what} खोज रहा था। वह खोज पृष्ठभूमि में चलती रही, {done} बजे पूरी हुई, और यह उत्तर उसका इस्तेमाल करता है।",
  "Show what it found": "क्या मिला, देखें",
  /* ---------- another conversation is changing the matter ---------- */
  "Waiting for '{title}' to finish, by {time}": "'{title}' के पूरा होने की प्रतीक्षा, {time} बजे तक",
  "Waiting for '{title}' to finish.": "'{title}' के पूरा होने की प्रतीक्षा है।",
  "One conversation changes a matter's papers at a time. Your message is ready to go.":
    "किसी मामले के दस्तावेज़ एक समय में एक ही बातचीत बदलती है। आपका संदेश भेजने के लिए तैयार है।",
  "Stop the other": "दूसरी रोकें",
} as const;

/** Every string of living in matters, in English. */
export type MatterKey = keyof typeof HI;

/** A string of living in matters in the language, its slots filled. */
export const mt = wordsOf<MatterKey>(HI);

/** Every key, for the banned-words screen in the tests. */
export const matterWords: Readonly<Record<MatterKey, string>> = HI;

/** The words of living in matters, bound to the page's language, with the shared, interface and frame words beside
 * them: const w = useMatterWords(); w.m("Send now"), w.t("Matters"), w.f("Pinned"). */
export function useMatterWords() {
  const { lang } = useDisplay();
  return {
    lang,
    m: (key: MatterKey, slots?: Slots) => mt(lang, key, slots),
    t: (key: Key, slots?: Slots) => roomWords(lang, key, slots),
    ui: (key: UiKey, slots?: Slots) => ui(lang, key, slots),
    f: (key: FrameKey, slots?: Slots) => frame(lang, key, slots),
    clock: (time: string) => clock(lang, time),
  };
}
