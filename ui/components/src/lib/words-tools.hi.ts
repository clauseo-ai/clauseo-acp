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
  // under the browser's Install Chrome (or Chrome or Edge, on Windows), in full: the browser's skill loads as each
  // conversation starts, so a browser installed later is there in the next new conversation
  // (provisional Hindi, app track final)
  "Install Google Chrome, then start a new conversation.":
    "Google Chrome इंस्टॉल करें, फिर एक नई बातचीत शुरू करें।",
  "Install Google Chrome or Microsoft Edge, then start a new conversation.":
    "Google Chrome या Microsoft Edge इंस्टॉल करें, फिर एक नई बातचीत शुरू करें।",
  // a policy read found nothing blocking: the browser row's reason, and its tooltip on when the browser comes
  "Ready for new conversations": "नई बातचीत के लिए तैयार",
  "The browser comes with your next conversation.": "ब्राउज़र आपकी अगली बातचीत के साथ आएगा।",
  // under the browser whose policy Clauseo couldn't check: read it again now
  "Try again": "फिर से प्रयास करें",
  // under the browser's reason when the firm's policy blocks it, in full: installing a browser doesn't help
  // (provisional Hindi, app track final)
  "A Chrome or Edge policy on this computer doesn't let apps control the browser, so Clauseo can't use one for you.":
    "इस कंप्यूटर पर Chrome या Edge की एक नीति ऐप्स को ब्राउज़र नियंत्रित नहीं करने देती, इसलिए Clauseo आपके लिए ब्राउज़र का उपयोग नहीं कर सकता।",
  // the same on Windows, where the host passes over a browser its own policy blocks, so only every installed one
  // blocked shows it, naming those: both (UC-550), Edge alone (UC-552), Chrome alone (UC-553); what to do, ask the
  // firm's IT (provisional Hindi, app track final)
  "Your firm's settings don't let apps control Microsoft Edge or Google Chrome. Ask your firm's IT to allow this for Clauseo, then start a new conversation.":
    "आपकी फ़र्म की सेटिंग्स ऐप्स को Microsoft Edge या Google Chrome नियंत्रित नहीं करने देतीं। अपनी फ़र्म की IT टीम से Clauseo के लिए इसकी अनुमति देने को कहें, फिर एक नई बातचीत शुरू करें।",
  "Your firm's settings don't let apps control Microsoft Edge. Ask your firm's IT to allow this for Clauseo, then start a new conversation.":
    "आपकी फ़र्म की सेटिंग्स ऐप्स को Microsoft Edge नियंत्रित नहीं करने देतीं। अपनी फ़र्म की IT टीम से Clauseo के लिए इसकी अनुमति देने को कहें, फिर एक नई बातचीत शुरू करें।",
  "Your firm's settings don't let apps control Google Chrome. Ask your firm's IT to allow this for Clauseo, then start a new conversation.":
    "आपकी फ़र्म की सेटिंग्स ऐप्स को Google Chrome नियंत्रित नहीं करने देतीं। अपनी फ़र्म की IT टीम से Clauseo के लिए इसकी अनुमति देने को कहें, फिर एक नई बातचीत शुरू करें।",
  // Windows, the only Chrome a per-user copy, which Clauseo's browser can't start: what would work instead
  // (UC-551; provisional Hindi, app track final)
  "Clauseo can't use a copy of Google Chrome installed just for you. Install Microsoft Edge, or ask your firm's IT to install Google Chrome for everyone on this computer. Then start a new conversation.":
    "Clauseo सिर्फ़ आपके लिए इंस्टॉल की गई Google Chrome की कॉपी का उपयोग नहीं कर सकता। Microsoft Edge इंस्टॉल करें, या अपनी फ़र्म की IT टीम से इस कंप्यूटर पर सभी के लिए Google Chrome इंस्टॉल करने को कहें। फिर एक नई बातचीत शुरू करें।",
  // under the browser's reason when Clauseo couldn't read that policy, in full
  // (provisional Hindi, app track final)
  "Clauseo couldn't check this computer's browser settings, so it won't use a browser for now. It checks again the next time it needs one.":
    "Clauseo इस कंप्यूटर की ब्राउज़र सेटिंग्स की जाँच नहीं कर सका, इसलिए वह अभी ब्राउज़र का उपयोग नहीं करेगा। अगली बार ब्राउज़र की ज़रूरत होने पर वह फिर से जाँच करेगा।",
  // under the browser's reason when Clauseo couldn't start its safe connection, in full
  // (provisional Hindi, app track final)
  "Clauseo couldn't start its safe connection for the browser, so it won't use one for now. It tries again when a new conversation starts.":
    "Clauseo ब्राउज़र के लिए अपना सुरक्षित कनेक्शन शुरू नहीं कर सका, इसलिए वह अभी ब्राउज़र का उपयोग नहीं करेगा। नई बातचीत शुरू होने पर वह फिर से प्रयास करेगा।",
  // under the browser's reason when Clauseo's unsafe-site list didn't load, in full
  // (provisional Hindi, app track final)
  "Clauseo's unsafe-site list didn't load, so it won't use a browser for now. It tries again when a new conversation starts.":
    "Clauseo की असुरक्षित वेबसाइटों की सूची लोड नहीं हुई, इसलिए वह अभी ब्राउज़र का उपयोग नहीं करेगा। नई बातचीत शुरू होने पर वह फिर से प्रयास करेगा।",
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsWords: Hindi<keyof typeof HI> = HI;
