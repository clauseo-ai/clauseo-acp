// The Hindi of words-tools.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-tools.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // each row's state chip
  On: "चालू",
  Off: "बंद",
  // the popover's foot when a tool is off in Settings, and its tooltip: a change never reaches the open conversation
  "Open Settings": "सेटिंग्स खोलें",
  "Changes apply to new conversations.": "बदलाव नई बातचीत पर लागू होते हैं।",
  // under the browser's Install Chrome (or Chrome or Edge, on Windows), in full: the browser's skill loads as Clauseo
  // starts, so a browser installed later needs Clauseo reopened
  // (provisional Hindi, app track final)
  "Install Google Chrome, then close and reopen Clauseo.":
    "Google Chrome इंस्टॉल करें, फिर Clauseo बंद करके दोबारा खोलें।",
  "Install Google Chrome or Microsoft Edge, then close and reopen Clauseo.":
    "Google Chrome या Microsoft Edge इंस्टॉल करें, फिर Clauseo बंद करके दोबारा खोलें।",
  // a policy read found nothing blocking: the browser row's reason, and its tooltip on when the browser comes
  "Ready for new conversations": "नई बातचीत के लिए तैयार",
  "The browser comes with your next conversation.": "ब्राउज़र आपकी अगली बातचीत के साथ आएगा।",
  // under the browser whose policy Clauseo couldn't check: read it again now
  "Try again": "फिर से प्रयास करें",
  // under the browser's reason when the firm's policy blocks it, in full: installing a browser doesn't help
  // (provisional Hindi, app track final)
  "Your firm doesn't allow apps to control a browser on this computer, so Clauseo can't use one for you.":
    "आपकी फ़र्म इस कंप्यूटर पर ऐप्स को ब्राउज़र नियंत्रित करने की अनुमति नहीं देती, इसलिए Clauseo आपके लिए ब्राउज़र का उपयोग नहीं कर सकता।",
  // under the browser's reason when Clauseo couldn't read that policy, in full
  // (provisional Hindi, app track final)
  "Clauseo couldn't check this computer's browser settings, so it won't use a browser for now. It checks again the next time it needs one.":
    "Clauseo इस कंप्यूटर की ब्राउज़र सेटिंग्स की जाँच नहीं कर सका, इसलिए वह अभी ब्राउज़र का उपयोग नहीं करेगा। अगली बार ब्राउज़र की ज़रूरत होने पर वह फिर से जाँच करेगा।",
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsWords: Hindi<keyof typeof HI> = HI;
