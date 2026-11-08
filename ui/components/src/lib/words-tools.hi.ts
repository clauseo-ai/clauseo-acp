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
  // the Chrome download; a number and its unit are held together by a non-breaking space (\u00a0)
  "Download Chrome": "Chrome डाउनलोड करें",
  "200\u00a0MB": "200\u00a0MB",
  "From Google, once. Needs 1\u00a0GB free.": "Google से, एक बार। 1\u00a0GB खाली जगह चाहिए।",
  "Install Chrome to use it": "इसके लिए Chrome इंस्टॉल करें",
  "{done} of {total}\u00a0MB": "{total}\u00a0MB में से {done}",
  Unpacking: "अनपैक हो रहा है",
  // Chrome is in: the browser row's reason, and its tooltip on when the browser comes
  "Ready for new conversations": "नई बातचीत के लिए तैयार",
  "The browser comes with your next conversation.": "ब्राउज़र आपकी अगली बातचीत के साथ आएगा।",
  // a failed download, by the host's code
  "Connection dropped": "कनेक्शन टूट गया",
  "File didn't match": "फ़ाइल मेल नहीं खाई",
  "Needs 1\u00a0GB free": "1\u00a0GB खाली जगह चाहिए",
  "Network blocked it": "नेटवर्क ने रोक दिया",
  "Couldn't unpack": "अनपैक नहीं हो सका",
  "Download failed": "डाउनलोड विफल",
  // no verified list of Chrome versions yet, so nothing was downloaded; Clauseo fetches it on its own
  // (provisional Hindi, app track final)
  "Not ready to download yet": "अभी डाउनलोड के लिए तैयार नहीं",
  "Try again": "फिर से प्रयास करें",
  // under the browser's reason when the firm's policy blocks it, in full: no download helps
  // (provisional Hindi, app track final)
  "Your firm doesn't allow apps to control a browser on this computer, so Clauseo can't use one for you.":
    "आपकी फ़र्म इस कंप्यूटर पर ऐप्स को ब्राउज़र नियंत्रित करने की अनुमति नहीं देती, इसलिए Clauseo आपके लिए ब्राउज़र का उपयोग नहीं कर सकता।",
  // under the browser's reason when Clauseo couldn't read that policy, in full
  // (provisional Hindi, app track final)
  "Clauseo couldn't check this computer's browser settings, so it won't use a browser for now. It checks again the next time it needs one.":
    "Clauseo इस कंप्यूटर की ब्राउज़र सेटिंग्स की जाँच नहीं कर सका, इसलिए वह अभी ब्राउज़र का उपयोग नहीं करेगा। अगली बार ब्राउज़र की ज़रूरत होने पर वह फिर से जाँच करेगा।",
  // under the browser's reason when Clauseo has no verified list of Chrome versions, in full: no download helps
  // (provisional Hindi, app track final)
  "Clauseo couldn't check that its copy of Chrome is up to date, so it won't use a browser for now. It keeps trying on its own.":
    "Clauseo यह जाँच नहीं कर सका कि उसका Chrome अप-टू-डेट है या नहीं, इसलिए वह अभी ब्राउज़र का उपयोग नहीं करेगा। वह अपने आप कोशिश करता रहेगा।",
  // under a browser that is on, when Clauseo's newest list of Chrome versions is old: a note, nothing is wrong
  // (provisional Hindi, app track final)
  "Clauseo hasn't checked for a newer Chrome in a while. The browser still works.":
    "Clauseo ने कुछ समय से नए Chrome की जाँच नहीं की है। ब्राउज़र अब भी काम करता है।",
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsWords: Hindi<keyof typeof HI> = HI;
