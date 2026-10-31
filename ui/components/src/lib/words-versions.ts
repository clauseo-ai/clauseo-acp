// The words of Versions, the whole matter as it stood at a time, with Compare, Restore and Undo, and of the edge of
// the matter: asking before going beyond it, the computer's own dialogs, and Not protected. As in words.ts the
// English is the key, the exact string on screen, and a key with slots fills them in either language; {time} takes
// clock(), so Hindi puts the part of the day first. The Hindi uses the formal आप, the danda, Western digits and
// English names in Latin script. Nothing here says I or मैं, or pencil, ink, step, पेंसिल, स्याही, चरण or कदम.
import { clock, t as roomWords, type Key } from "./words.ts";
import { ui, type UiKey } from "./words-ui.ts";
import { useDisplay, type Lang } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";

const HI = {
  // the name of the thing, and its places
  Versions: "संस्करण",
  "{n} versions": "{n} संस्करण",
  "1 version": "1 संस्करण",
  Now: "अभी",
  Today: "आज",
  "The matter's versions are kept on this computer.": "मामले के संस्करण इसी कंप्यूटर पर रखे जाते हैं।",
  // the timeline, with hundreds of versions
  "Show versions by": "किसके संस्करण दिखाएँ",
  All: "सभी",
  "By Clauseo": "Clauseo के",
  "By you": "आपके",
  "{n} more · {from} to {to}": "{n} और · {from} से {to} तक",
  "Earlier, {span}": "पहले के, {span}",
  "{from} to {to}": "{from} से {to}",
  "{file} is too large to keep versions": "{file} इतनी बड़ी है कि उसके संस्करण नहीं रखे जा सकते",
  // the plain line over the room as it stood, and its ways on
  "The matter as it stood at {time} on {date}": "मामला जैसा {date} को {time} बजे था",
  "The matter as it stood at {time} today": "मामला जैसा आज {time} बजे था",
  "Compare with now": "अभी से तुलना करें",
  "Restore this version": "यह संस्करण पुनर्स्थापित करें",
  "Undo this change": "यह बदलाव पूर्ववत करें",
  "Back to now": "अभी पर लौटें",
  // who made a version
  "Clauseo, in {conversation}": "Clauseo, '{conversation}' में",
  "You, outside Clauseo": "आप, Clauseo के बाहर",
  "Undo of the change at {time}": "{time} बजे का बदलाव पूर्ववत किया गया",
  // the approved sentence without its time, for an undo whose version isn't in the history at hand
  "Undo of a change": "बदलाव पूर्ववत किया गया",
  "Undo of the change at {time} on {date}": "{date} को {time} बजे का बदलाव पूर्ववत किया गया",
  "Imported {n} documents": "{n} दस्तावेज़ आयात किए गए",
  "Restore to {time} on {date}": "{date}, {time} बजे पर पुनर्स्थापित",
  "Restore to {time} today": "आज {time} बजे पर पुनर्स्थापित",
  "Undo of a restore": "पुनर्स्थापन पूर्ववत किया गया",
  "Undo of the restore at {time}": "{time} बजे का पुनर्स्थापन पूर्ववत किया गया",
  "Undo of the restore at {time} on {date}": "{date} को {time} बजे का पुनर्स्थापन पूर्ववत किया गया",
  "Restore of a version": "संस्करण पुनर्स्थापित किया गया",
  // what a version did to the papers
  "Changed {papers}.": "बदले गए: {papers}।",
  "Added {papers}.": "जोड़े गए: {papers}।",
  "Made {papers}.": "बनाए गए: {papers}।",
  // the papers in a version, on the wall
  "Not in the matter yet": "तब तक मामले में नहीं था",
  "Changed in this version": "इस संस्करण में बदला गया",
  // a file too large to keep versions
  "{file} is {size}, too large to keep versions. Versions keep files up to {cap}. Clauseo can read it but not change it, and Restore and Undo leave it as it is.":
    "{file} {size} की है, संस्करण रखने के लिए बहुत बड़ी। संस्करण {cap} तक की फ़ाइलों के रखे जाते हैं। Clauseo इसे पढ़ सकता है पर बदल नहीं सकता, और पुनर्स्थापन व पूर्ववत इसे जैसी है वैसी ही छोड़ते हैं।",
  // compare
  "Documents that differ from now": "जो दस्तावेज़ अब अलग हैं",
  "A PDF shows only whether it changed. Open each to read it.":
    "PDF के बारे में केवल इतना दिखता है कि वह बदला या नहीं। पढ़ने के लिए हर एक को खोलें।",
  "As it is now": "जैसा अभी है",
  "Open as it was": "जैसा था वैसा खोलें",
  "Open now": "अभी वाला खोलें",
  "{title} did not exist yet.": "{title} तब तक बनी नहीं थी।",
  // over the sheet, and saving it as a new draft
  "{paper}, as it stood at {time} on {date}": "{paper}, जैसा {date} को {time} बजे था",
  "{paper}, as it stood at {time} today": "{paper}, जैसा आज {time} बजे था",
  "{paper}, as it stands now": "{paper}, जैसा अभी है",
  // restoring
  "Restore the matter to how it was at {time} on {date}?":
    "मामले को वैसा पुनर्स्थापित करें जैसा वह {date} को {time} बजे था?",
  "Restore the matter to how it was at {time} today?":
    "मामले को वैसा पुनर्स्थापित करें जैसा वह आज {time} बजे था?",
  "{n} documents will change: {papers}. You can undo this.":
    "{n} दस्तावेज़ बदलेंगे: {papers}। आप इसे पूर्ववत कर सकते हैं।",
  "1 document will change: {papers}. You can undo this.":
    "1 दस्तावेज़ बदलेगा: {papers}। आप इसे पूर्ववत कर सकते हैं।",
  "Nothing will change. The matter already stands as it did then.":
    "कुछ नहीं बदलेगा। मामला पहले से वैसा ही है जैसा तब था।",
  "'{papers}', added since, stays.": "बाद में जोड़ा गया '{papers}' बना रहेगा।",
  "Close '{file}' in Word to restore it.": "पुनर्स्थापित करने के लिए Word में '{file}' बंद करें।",
  "Close '{file}' in Word to undo the change.": "बदलाव पूर्ववत करने के लिए Word में '{file}' बंद करें।",
  "Word holds the file while it is open. Nothing has changed yet.":
    "खुली रहने तक फ़ाइल Word के पास रहती है। अभी कुछ नहीं बदला है।",
  "Try again": "फिर से प्रयास करें",
  "Restore and Undo wait until the answer in {conversation} is done, expected by {time}.":
    "'{conversation}' का उत्तर पूरा होने तक पुनर्स्थापन और पूर्ववत प्रतीक्षा करेंगे। {time} बजे तक अपेक्षित।",
  "Restored to {time} on {date}. {n} documents changed.":
    "{date}, {time} बजे पर पुनर्स्थापित किया गया। {n} दस्तावेज़ बदले।",
  "Restored to {time} on {date}. 1 document changed.":
    "{date}, {time} बजे पर पुनर्स्थापित किया गया। 1 दस्तावेज़ बदला।",
  "Restored to {time} today. {n} documents changed.":
    "आज {time} बजे पर पुनर्स्थापित किया गया। {n} दस्तावेज़ बदले।",
  "Restored to {time} today. 1 document changed.":
    "आज {time} बजे पर पुनर्स्थापित किया गया। 1 दस्तावेज़ बदला।",
  "Undid the restore. The matter stands as it did at {time}.":
    "पुनर्स्थापन पूर्ववत किया गया। मामला वैसा है जैसा {time} बजे था।",
  // undoing one change
  "Undo the change Clauseo made at {time}?": "{time} बजे Clauseo का किया बदलाव पूर्ववत करें?",
  "Undo the change you made outside Clauseo at {time}?":
    "Clauseo के बाहर {time} बजे आपका किया बदलाव पूर्ववत करें?",
  "{n} documents go back to how they were just before it: {papers}. The undo is kept as a version of its own.":
    "{n} दस्तावेज़ वैसे हो जाएँगे जैसे उससे ठीक पहले थे: {papers}। पूर्ववत करना भी एक अलग संस्करण के रूप में रखा जाता है।",
  "1 document goes back to how it was just before it: {papers}. The undo is kept as a version of its own.":
    "1 दस्तावेज़ वैसा हो जाएगा जैसा उससे ठीक पहले था: {papers}। पूर्ववत करना भी एक अलग संस्करण के रूप में रखा जाता है।",
  "Nothing goes back. Every document it changed is kept as it is now.":
    "कुछ नहीं लौटेगा। इसके बदले हर दस्तावेज़ को जैसा अभी है वैसा रखा जाएगा।",
  "{n} documents changed again after it.": "इसके बाद {n} दस्तावेज़ फिर बदले गए।",
  "1 document changed again after it.": "इसके बाद 1 दस्तावेज़ फिर बदला गया।",
  "Changed by you, outside Clauseo, at {time}": "आपने Clauseo के बाहर {time} बजे बदला",
  "Changed by Clauseo at {time}": "Clauseo ने {time} बजे बदला",
  "For {title}": "{title} के लिए",
  "Undid the change at {time}. {papers} went back, and the undo is a version of its own.":
    "{time} बजे का बदलाव पूर्ववत किया गया। {papers} पहले जैसे हो गए, और यह पूर्ववत करना एक अलग संस्करण है।",
  // a version that couldn't be saved
  // where the history lives
  "Where versions are kept": "संस्करण कहाँ रखे जाते हैं",
  "On this computer, in Clauseo's own folder. Never in OneDrive, Dropbox or any folder that syncs, so syncing never touches them.":
    "इसी कंप्यूटर पर, Clauseo के अपने फ़ोल्डर में। कभी OneDrive, Dropbox या किसी सिंक होने वाले फ़ोल्डर में नहीं, इसलिए सिंक उन्हें कभी नहीं छूता।",
  "The matter's own folder can sync as it always has. Its versions stay here.":
    "मामले का अपना फ़ोल्डर पहले की तरह सिंक हो सकता है। उसके संस्करण यहीं रहते हैं।",
  "On another computer, this matter's versions start afresh.":
    "किसी दूसरे कंप्यूटर पर इस मामले के संस्करण नए सिरे से शुरू होते हैं।",
  "This matter's versions take {size}. They are kept for good.":
    "इस मामले के संस्करण {size} जगह लेते हैं। वे हमेशा के लिए रखे जाते हैं।",
  "Keep 90 days instead": "इसके बजाय 90 दिन रखें",
  // History, with the actor first
  "{time} on {date}": "{date}, {time}",
  "{time} today": "आज {time}",
  // a list of two, as a sentence joins them
  "{a} and {b}": "{a} और {b}",
} as const;

const EDGE = {
  // asking at the edge of the matter, in the thread
  "Clauseo wants to read a folder outside this matter: {place}.":
    "Clauseo इस मामले के बाहर का एक फ़ोल्डर पढ़ना चाहता है: {place}।",
  "Clauseo wants to open a website this matter hasn't used before: {place}.":
    "Clauseo एक ऐसी वेबसाइट खोलना चाहता है जो इस मामले में पहले इस्तेमाल नहीं हुई: {place}।",
  "Clauseo asks to work without protection in this matter, to read the scanned annexures with a program on this computer.":
    "Clauseo इस मामले में संरक्षण के बिना काम करने की अनुमति माँगता है, ताकि इस कंप्यूटर के एक प्रोग्राम से स्कैन किए गए अनुलग्नक पढ़ सके।",
  "Clauseo works inside this matter's folder and asks before going beyond it.":
    "Clauseo इस मामले के फ़ोल्डर के भीतर काम करता है और उससे बाहर जाने से पहले पूछता है।",
  "Without protection, Clauseo can reach any file on this computer without asking.":
    "संरक्षण के बिना Clauseo बिना पूछे इस कंप्यूटर की किसी भी फ़ाइल तक पहुँच सकता है।",
  Allow: "अनुमति दें",
  "Don't allow": "अनुमति न दें",
  "Keep protection": "संरक्षण बनाए रखें",
  "Switch off": "बंद करें",
  "macOS is asking you to confirm.": "macOS आपसे पुष्टि माँग रहा है।",
  "Windows is asking you to confirm.": "Windows आपसे पुष्टि माँग रहा है।",
  // answered, in the past tense
  "Allowed for this matter. Clauseo can read {place}.":
    "इस मामले के लिए अनुमति दी गई। Clauseo {place} पढ़ सकता है।",
  "Allowed this time. Clauseo can read {place} for this answer.":
    "इस बार अनुमति दी गई। Clauseo इस उत्तर के लिए {place} पढ़ सकता है।",
  "Not allowed for this matter. Clauseo keeps to this matter's folder.":
    "इस मामले के लिए अनुमति नहीं दी गई। Clauseo इस मामले के फ़ोल्डर तक ही रहता है।",
  "Not allowed. Clauseo kept to this matter's folder.":
    "अनुमति नहीं दी गई। Clauseo इस मामले के फ़ोल्डर तक ही रहा।",
  "Allowed for this matter. Clauseo can open {place}.":
    "इस मामले के लिए अनुमति दी गई। Clauseo {place} खोल सकता है।",
  "Allowed this time. Clauseo can open {place} for this answer.":
    "इस बार अनुमति दी गई। Clauseo इस उत्तर के लिए {place} खोल सकता है।",
  "Not allowed for this matter. Clauseo won't open {place}.":
    "इस मामले के लिए अनुमति नहीं दी गई। Clauseo {place} नहीं खोलेगा।",
  "Not allowed. Clauseo didn't open {place}.": "अनुमति नहीं दी गई। Clauseo ने {place} नहीं खोली।",
  "Protection switched off for this matter at {time}.":
    "{time} बजे इस मामले के लिए संरक्षण बंद किया गया।",
  "Protection kept. Clauseo left the scanned annexures unread.":
    "संरक्षण बना रहा। Clauseo ने स्कैन किए गए अनुलग्नक नहीं पढ़े।",
  // the computer's own dialogs
  // the working line while the edge waits, and while Claude starts again
  // after refusals
  "Clauseo stopped to ask you. Its last 3 tries went beyond this matter and were refused.":
    "Clauseo आपसे पूछने के लिए रुका है। उसकी पिछली 3 कोशिशें इस मामले से बाहर गईं और रोकी गईं।",
  "Clauseo stopped to ask you. 20 tries in this answer went beyond this matter and were refused.":
    "Clauseo आपसे पूछने के लिए रुका है। इस उत्तर में 20 कोशिशें इस मामले से बाहर गईं और रोकी गईं।",
  "Nothing beyond the matter was read or changed.": "मामले के बाहर कुछ भी पढ़ा या बदला नहीं गया।",
  "What was refused": "क्या रोका गया",
  "{n} earlier": "{n} पहले के",
  "Go on inside the matter": "मामले के भीतर ही आगे बढ़ें",
  "Stop the answer": "उत्तर रोकें",
  "You told Clauseo to go on inside the matter.": "आपने Clauseo से मामले के भीतर ही आगे बढ़ने को कहा।",
  "You stopped the answer at {time}. The answer so far is kept.":
    "आपने {time} बजे उत्तर रोका। अब तक का उत्तर रखा गया है।",
  // what this matter allows
  "What this matter allows": "यह मामला किसकी अनुमति देता है",
  Always: "हमेशा",
  "Allowed for this matter": "इस मामले के लिए अनुमति",
  "Refused for this matter": "इस मामले के लिए मना",
  "This matter's folder": "इस मामले का फ़ोल्डर",
  "Read and change": "पढ़ना और बदलना",
  "Your Claude plan": "आपका Claude प्लान",
  "Your questions, and the parts of the papers they need":
    "आपके प्रश्न, और दस्तावेज़ों के वे अंश जो उनके लिए ज़रूरी हैं",
  "Searches for judgments, without your papers": "निर्णयों की खोज, आपके दस्तावेज़ों के बिना",
  Remove: "हटाएँ",
  "Removed. Clauseo asks again before using it.":
    "हटा दिया गया। इस्तेमाल करने से पहले Clauseo फिर पूछेगा।",
  Undo: "पूर्ववत करें",
  Protection: "संरक्षण",
  "On. Clauseo works inside this matter's folder and asks before going beyond it.":
    "चालू। Clauseo इस मामले के फ़ोल्डर के भीतर काम करता है और उससे बाहर जाने से पहले पूछता है।",
  "Off since {time} today. Clauseo can reach any file on this computer without asking.":
    "आज {time} बजे से बंद। Clauseo बिना पूछे इस कंप्यूटर की किसी भी फ़ाइल तक पहुँच सकता है।",
  "Couldn't be set up on this computer. Clauseo asks before each change in this matter.":
    "इस कंप्यूटर पर सेट नहीं हो सका। Clauseo इस मामले में हर बदलाव से पहले पूछता है।",
  "Clauseo asks before each change in this matter.": "Clauseo इस मामले में हर बदलाव से पहले पूछता है।",
  "Protection couldn't be set up on this computer, so Clauseo asks before each change in this matter.":
    "इस कंप्यूटर पर संरक्षण सेट नहीं हो सका, इसलिए Clauseo इस मामले में हर बदलाव से पहले पूछता है।",
  "Switch off protection": "संरक्षण बंद करें",
  "Switch protection on": "संरक्षण चालू करें",
  "Safe workspace. Clauseo works inside this matter's folder and asks before going beyond it.":
    "सुरक्षित कार्यक्षेत्र। Clauseo इस मामले के फ़ोल्डर के भीतर काम करता है और उससे बाहर जाने से पहले पूछता है।",
  // not protected
  "Not protected": "संरक्षित नहीं",
  "Not protected.": "संरक्षित नहीं।",
  "Clauseo can reach beyond this matter's folder without asking.":
    "Clauseo बिना पूछे इस मामले के फ़ोल्डर से बाहर पहुँच सकता है।",
  "This matter is not protected": "यह मामला संरक्षित नहीं है",
  "Protection is off for {matter}. Clauseo can read and change files anywhere on this computer while it works here, without asking first.":
    "{matter} के लिए संरक्षण बंद है। यहाँ काम करते समय Clauseo बिना पहले पूछे इस कंप्यूटर पर कहीं भी फ़ाइलें पढ़ और बदल सकता है।",
  "Windows didn't allow the protected space on this computer.":
    "Windows ने इस कंप्यूटर पर संरक्षित स्थान की अनुमति नहीं दी।",
  "macOS didn't allow the protected space on this computer.":
    "macOS ने इस कंप्यूटर पर संरक्षित स्थान की अनुमति नहीं दी।",
  "Every change inside the matter's folder is still kept, and Versions can undo it.":
    "मामले के फ़ोल्डर के भीतर हर बदलाव अब भी रखा जाता है, और संस्करण उसे पूर्ववत कर सकते हैं।",
  Understood: "ठीक है",
  // the computer's own dialogs, as edgeDialogWords builds them
  "Switch off protection for {matter}?": "{matter} के लिए संरक्षण बंद करें?",
  "If you switch it off, Clauseo can read and change any file on this computer while it works on this matter, without asking.":
    "बंद करने पर Clauseo इस मामले पर काम करते समय बिना पूछे इस कंप्यूटर की कोई भी फ़ाइल पढ़ और बदल सकता है।",
  "If you keep it, Clauseo stays inside this matter's folder, and the scanned annexures stay unread.":
    "बनाए रखने पर Clauseo इस मामले के फ़ोल्डर के भीतर रहता है, और स्कैन किए गए अनुलग्नक बिना पढ़े रहते हैं।",
  "You can switch it back on from the line under the question box.":
    "आप इसे प्रश्न बॉक्स के नीचे वाली पंक्ति से फिर चालू कर सकते हैं।",
  "If you allow it, Clauseo can read every file in {name} for this matter, and what it reads goes to Claude as your papers do.":
    "अनुमति देने पर Clauseo इस मामले के लिए {name} की हर फ़ाइल पढ़ सकता है, और जो वह पढ़ता है वह आपके दस्तावेज़ों की तरह Claude को जाता है।",
  "If you don't, Clauseo keeps to this matter's folder and answers without {name}.":
    "अनुमति न देने पर Clauseo इस मामले के फ़ोल्डर तक ही रहता है और {name} के बिना उत्तर देता है।",
  "If you allow it, Clauseo can read pages on {place} and send it searches for this matter.":
    "अनुमति देने पर Clauseo इस मामले के लिए {place} के पृष्ठ पढ़ सकता है और उसे खोज भेज सकता है।",
  "If you don't, Clauseo answers from your papers and Indian Kanoon, without this website.":
    "अनुमति न देने पर Clauseo इस वेबसाइट के बिना, आपके दस्तावेज़ों और Indian Kanoon से उत्तर देता है।",
  "Allow Clauseo to read the folder '{name}'?": "Clauseo को फ़ोल्डर '{name}' पढ़ने की अनुमति दें?",
  "Allow Clauseo to open {place}?": "Clauseo को {place} खोलने की अनुमति दें?",
  "Remember for this matter": "इस मामले के लिए याद रखें",
} as const;

/** Every string of Versions, in English: the matter as it stood, and the edge of the matter. */
const ALL = { ...HI, ...EDGE } as const;
/** Every string, English to Hindi, for the tests' banned-words screen. */
export const VERSION_WORDS: Readonly<Record<string, string>> = ALL;
export type VersionKey = keyof typeof ALL;

/** A string of Versions in the language, its slots filled. */
export const vt = wordsOf<VersionKey>(ALL);

/** "A", "A and B", "A, B and C", in the language. */
export function listOf(lang: Lang, items: readonly string[]): string {
  if (items.length <= 1) return items[0] ?? "";
  const head = items.slice(0, -1).join(", ");
  return vt(lang, "{a} and {b}", { a: head, b: items[items.length - 1] ?? "" });
}

/** Versions' words of the page's language, bound, with the room's and the shared interface words beside them. */
export function useVersionWords() {
  const { lang } = useDisplay();
  return {
    lang,
    v: (key: VersionKey, slots?: Slots) => vt(lang, key, slots),
    t: (key: Key, slots?: Slots) => roomWords(lang, key, slots),
    ui: (key: UiKey, slots?: Slots) => ui(lang, key, slots),
    clock: (time: string) => clock(lang, time),
    list: (items: readonly string[]) => listOf(lang, items),
  };
}

/** What the computer's own dialog says at each edge, for OsDialogPreview. name: the folder's last part, "Precedents";
 * place: a website, "example.org"; matter: the matter's name as filed. */
export type EdgeAsk =
  | { kind: "folder"; name: string }
  | { kind: "site"; place: string }
  | { kind: "off"; matter: string };

export function edgeDialogWords(lang: Lang, ask: EdgeAsk) {
  const v = (key: VersionKey, slots?: Slots) => vt(lang, key, slots);
  if (ask.kind === "off") {
    return {
      title: v("Switch off protection for {matter}?", { matter: ask.matter }),
      lines: [
        v(
          "If you switch it off, Clauseo can read and change any file on this computer while it works on this matter, without asking.",
        ),
        v(
          "If you keep it, Clauseo stays inside this matter's folder, and the scanned annexures stay unread.",
        ),
      ] as const,
      foot: v("You can switch it back on from the line under the question box."),
      remember: null,
      confirm: v("Switch off"),
      cancel: v("Keep protection"),
      primary: "cancel" as const,
    };
  }
  const lines =
    ask.kind === "folder"
      ? ([
          v(
            "If you allow it, Clauseo can read every file in {name} for this matter, and what it reads goes to Claude as your papers do.",
            { name: ask.name },
          ),
          v("If you don't, Clauseo keeps to this matter's folder and answers without {name}.", {
            name: ask.name,
          }),
        ] as const)
      : ([
          v(
            "If you allow it, Clauseo can read pages on {place} and send it searches for this matter.",
            { place: ask.place },
          ),
          v(
            "If you don't, Clauseo answers from your papers and Indian Kanoon, without this website.",
          ),
        ] as const);
  return {
    title:
      ask.kind === "folder"
        ? v("Allow Clauseo to read the folder '{name}'?", { name: ask.name })
        : v("Allow Clauseo to open {place}?", { place: ask.place }),
    lines,
    foot: null,
    remember: v("Remember for this matter"),
    confirm: v("Allow"),
    cancel: v("Don't allow"),
    primary: "confirm" as const,
  };
}
