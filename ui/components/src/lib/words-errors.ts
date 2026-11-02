// The words for every error the host can return, chosen by its code and never its message (the message is for logs):
// one map from code to words. They stand on the working line after a refused send, and beside Undo after a refused undo.
// Each says what happened in plain words, and what stays kept where that is true. As in words.ts the English is the
// key, the exact string on screen. The Hindi is a specialist's draft, waiting for a Hindi-speaking lawyer: the formal
// आप, the danda, Western digits, English names in Latin script. Nothing here says मैं, पेंसिल, स्याही, चरण or कदम.
import type { Lang } from "./display.ts";
import { wordsOf } from "./words-core.ts";
import { th } from "./words-thread.ts";

/** Why a host call failed: the protocol's `ErrorCode` (ui/app/src/transport/bindings.ts), copied, since the parts
 * can't import the app. ui/app/src/room/thread/error-codes.test.ts fails when the two differ. */
export type ErrorCode =
  | "internal"
  | "command.invalid"
  | "command.conflict"
  | "command.unknown_matter"
  | "command.unknown_thread"
  | "thread.turn_running"
  | "thread.interrupted"
  | "thread.turn_not_queued"
  | "thread.queue_full"
  | "permission.not_pending"
  | "permission.unknown_option"
  | "elicitation.not_pending"
  | "elicitation.invalid_values"
  | "agent.launch_failed"
  | "agent.exited"
  | "agent.protocol_error"
  | "agent.auth_required"
  | "agent.unsupported_version"
  | "store.disk_full"
  | "store.corrupt"
  | "store.io"
  | "store.locked"
  | "store.newer_version"
  | "matter.checkpoint_failed"
  | "matter.folder_missing"
  | "matter.folder_refused"
  | "matter.file_locked"
  | "matter.turn_running"
  | "transport.unauthorized"
  | "transport.too_large"
  | "settings.locked"
  | "unknown";

const HI = {
  "Something went wrong inside Clauseo.": "Clauseo के भीतर कुछ गड़बड़ हो गई।",
  "Clauseo couldn't take that request as it was.": "Clauseo वह अनुरोध उस रूप में नहीं ले सका।",
  "That request clashes with one already made, so nothing changed.":
    "यह अनुरोध पहले किए गए अनुरोध से टकराता है, इसलिए कुछ नहीं बदला।",
  "This matter is no longer in Clauseo.": "यह मामला अब Clauseo में नहीं है।",
  "This conversation is no longer in Clauseo.": "यह बातचीत अब Clauseo में नहीं है।",
  "Wait for the answer to finish, or stop it.": "उत्तर पूरा होने तक प्रतीक्षा करें, या उसे रोकें।",
  "The answer to that question has already started or finished.":
    "उस प्रश्न का उत्तर पहले ही शुरू या पूरा हो चुका है।",
  "Too many questions are waiting in this conversation.": "इस बातचीत में बहुत अधिक प्रश्न प्रतीक्षा में हैं।",
  "That request was already answered, or it has expired.":
    "उस अनुरोध का उत्तर पहले ही दिया जा चुका है, या उसका समय निकल गया है।",
  "That answer wasn't one of the choices offered.": "वह उत्तर दिए गए विकल्पों में से नहीं था।",
  "That question was already answered, or it has expired.":
    "उस प्रश्न का उत्तर पहले ही दिया जा चुका है, या उसका समय निकल गया है।",
  "Those answers don't fit the question.": "ये उत्तर प्रश्न से मेल नहीं खाते।",
  "Claude didn't start. Your question is kept.": "Claude शुरू नहीं हुआ। आपका प्रश्न रखा गया है।",
  "Claude sent something Clauseo can't read.": "Claude ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
  "Sign in to Claude first. Your question is kept.":
    "पहले Claude में साइन इन करें। आपका प्रश्न रखा गया है।",
  "This version of Claude Code doesn't work with Clauseo.":
    "Claude Code का यह संस्करण Clauseo के साथ काम नहीं करता।",
  "The computer's disk is full. Free some space, then try again.":
    "कंप्यूटर की डिस्क भर गई है। कुछ जगह खाली करें, फिर दोबारा प्रयास करें।",
  "Clauseo couldn't read back its saved data.": "Clauseo अपना सहेजा गया डेटा वापस नहीं पढ़ सका।",
  "Clauseo couldn't read or write its data. Try again.":
    "Clauseo अपना डेटा पढ़ या लिख नहीं सका। फिर से प्रयास करें।",
  "Clauseo is already open in another window.": "Clauseo पहले से किसी दूसरी विंडो में खुला है।",
  "This data comes from a newer version of Clauseo.": "यह डेटा Clauseo के नए संस्करण से है।",
  "Clauseo couldn't save a version of this matter's papers, so this change can't be undone.":
    "Clauseo इस मामले के दस्तावेज़ों का संस्करण नहीं सहेज सका, इसलिए यह बदलाव पूर्ववत नहीं किया जा सकता।",
  "This matter's folder can't be found. It may have moved, or its drive isn't connected.":
    "इस मामले का फ़ोल्डर नहीं मिल रहा। हो सकता है वह कहीं और ले जाया गया हो, या उसकी ड्राइव जुड़ी न हो।",
  "A whole drive, your home folder or a system folder can't be a matter.":
    "पूरी ड्राइव, आपका होम फ़ोल्डर या कोई सिस्टम फ़ोल्डर मामला नहीं बन सकता।",
  "A paper is open in another program. Close it, then try again.":
    "कोई दस्तावेज़ किसी दूसरे प्रोग्राम में खुला है। उसे बंद करें, फिर दोबारा प्रयास करें।",
  "Wait for the answer to finish first.": "पहले उत्तर पूरा होने दें।",
  "That wasn't allowed, so nothing changed.": "इसकी अनुमति नहीं थी, इसलिए कुछ नहीं बदला।",
  "That is too large to send.": "यह भेजने के लिए बहुत बड़ा है।",
  "Your organisation manages that setting, so it can't be changed.":
    "यह सेटिंग आपका संगठन संभालता है, इसलिए इसे बदला नहीं जा सकता।",
} as const;

/** The thread's own sentence for an answer cut short: its Hindi lives in words-thread.ts, once. */
const STOPPED = "The answer stopped. Your question and the answer so far are kept." as const;

/** Every error sentence, in English. */
export type ErrorKey = keyof typeof HI | typeof STOPPED;

/** One sentence per code, in English. */
export const ERROR_WORDS: Record<ErrorCode, ErrorKey> = {
  internal: "Something went wrong inside Clauseo.",
  "command.invalid": "Clauseo couldn't take that request as it was.",
  "command.conflict": "That request clashes with one already made, so nothing changed.",
  "command.unknown_matter": "This matter is no longer in Clauseo.",
  "command.unknown_thread": "This conversation is no longer in Clauseo.",
  "thread.turn_running": "Wait for the answer to finish, or stop it.",
  "thread.interrupted": STOPPED,
  "thread.turn_not_queued": "The answer to that question has already started or finished.",
  "thread.queue_full": "Too many questions are waiting in this conversation.",
  "permission.not_pending": "That request was already answered, or it has expired.",
  "permission.unknown_option": "That answer wasn't one of the choices offered.",
  "elicitation.not_pending": "That question was already answered, or it has expired.",
  "elicitation.invalid_values": "Those answers don't fit the question.",
  "agent.launch_failed": "Claude didn't start. Your question is kept.",
  "agent.exited": STOPPED,
  "agent.protocol_error": "Claude sent something Clauseo can't read.",
  "agent.auth_required": "Sign in to Claude first. Your question is kept.",
  "agent.unsupported_version": "This version of Claude Code doesn't work with Clauseo.",
  "store.disk_full": "The computer's disk is full. Free some space, then try again.",
  "store.corrupt": "Clauseo couldn't read back its saved data.",
  "store.io": "Clauseo couldn't read or write its data. Try again.",
  "store.locked": "Clauseo is already open in another window.",
  "store.newer_version": "This data comes from a newer version of Clauseo.",
  "matter.checkpoint_failed":
    "Clauseo couldn't save a version of this matter's papers, so this change can't be undone.",
  "matter.folder_missing":
    "This matter's folder can't be found. It may have moved, or its drive isn't connected.",
  "matter.folder_refused": "A whole drive, your home folder or a system folder can't be a matter.",
  "matter.file_locked": "A paper is open in another program. Close it, then try again.",
  "matter.turn_running": "Wait for the answer to finish first.",
  "transport.unauthorized": "That wasn't allowed, so nothing changed.",
  "transport.too_large": "That is too large to send.",
  "settings.locked": "Your organisation manages that setting, so it can't be changed.",
  // a code from a newer Clauseo: as internal reads, since this version can say nothing truer
  unknown: "Something went wrong inside Clauseo.",
};

const own = wordsOf<keyof typeof HI>(HI);

/** An error sentence in the language. */
export function errorSentence(lang: Lang, key: ErrorKey): string {
  return key === STOPPED ? th(lang, key) : own(lang, key);
}

/** A code's sentence in the language; a code this version doesn't know reads as "unknown", which reads as "internal". */
export function errorWords(lang: Lang, code: string): string {
  const known = Object.hasOwn(ERROR_WORDS, code) ? ERROR_WORDS[code as ErrorCode] : null;
  return errorSentence(lang, known ?? ERROR_WORDS.unknown);
}
