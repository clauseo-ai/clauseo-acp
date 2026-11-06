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
  "Try again": "फिर से प्रयास करें",
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsWords: Hindi<keyof typeof HI> = HI;
