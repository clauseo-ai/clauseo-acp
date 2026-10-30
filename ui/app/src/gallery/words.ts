// The gallery's own sample words: labels for showing the controls in a realistic state, which no part uses. English
// is the key and Hindi the value, from the design's approved words, so the gallery can show the controls in Hindi.
// They live here, not in ui/components' words files, so the shipped words hold only what the parts use.
import { wordsOf, type Slots } from "@clauseo/components/lib/words-core.ts";
import type { FrameKey } from "@clauseo/components/lib/words-frame.ts";
import { useUiWords, type UiKey } from "@clauseo/components/lib/words-ui.ts";

const HI = {
  Rename: "नाम बदलें",
  "Questions go to this plan, which you already pay for.":
    "प्रश्न इसी प्लान पर जाते हैं, जिसका भुगतान आप पहले से करते हैं।",
  "Restore the {time} version of the petition?": "याचिका का {time} बजे वाला संस्करण पुनर्स्थापित करें?",
  "The current version stays in History.": "मौजूदा संस्करण इतिहास में बना रहता है।",
  "Lease.docx changed after Clauseo's edit.": "Clauseo के संपादन के बाद Lease.docx में बदलाव हुए हैं।",
  Plan: "प्लान",
  "Warn before adding a network folder": "नेटवर्क फ़ोल्डर जोड़ने से पहले चेतावनी दें",
  "Papers on a network drive take longer to read and to keep.":
    "नेटवर्क ड्राइव पर रखे दस्तावेज़ पढ़ने और सहेजने में अधिक समय लगता है।",
  "Lite mode": "सादा मोड",
  "Takes away every shadow and all motion.": "सभी छायाएँ और सारी हलचल हटा देता है।",
  Automatic: "स्वचालित",
  "On by itself over remote desktops such as Citrix": "Citrix जैसे रिमोट डेस्कटॉप पर अपने आप चालू",
  On: "चालू",
  "Always, on this computer": "इस कंप्यूटर पर हमेशा",
  Off: "बंद",
  "Never, even over a remote desktop": "कभी नहीं, रिमोट डेस्कटॉप पर भी नहीं",
  "Copy the list of citations": "उद्धृत निर्णयों की सूची कॉपी करें",
  "List of citations copied.": "उद्धृत निर्णयों की सूची कॉपी हो गई।",
  "Reading papers, {n} of {total}": "दस्तावेज़ पढ़े जा रहे हैं, {total} में से {n}",
  Folder: "फ़ोल्डर",
  "Clauseo reads only the papers in this folder.": "Clauseo केवल इसी फ़ोल्डर के दस्तावेज़ पढ़ता है।",
  "This folder has moved.": "यह फ़ोल्डर कहीं और ले जाया गया है।",
  Note: "टिप्पणी",
  "Shown under the matter's name in the list.": "सूची में मामले के नाम के नीचे दिखती है।",
  "Plenty left until the {time} reset": "{time} बजे के रीसेट तक काफ़ी बाकी है",
  "Close to the limit. Resets at {time}.": "सीमा के करीब। {time} बजे रीसेट होगा।",
  "Limit reached until {time}": "{time} बजे तक सीमा पूरी",
  Theme: "थीम",
  "Text size": "अक्षरों का आकार",
  "Reduced motion": "कम हलचल",
} as const;

type GalleryKey = keyof typeof HI;
const gallery = wordsOf<GalleryKey>(HI);
const isGallery = (key: string): key is GalleryKey => Object.hasOwn(HI, key);

/** The controls' words, as useUiWords gives them, with the gallery's samples beside them. */
export function useGalleryWords() {
  const w = useUiWords();
  return {
    ...w,
    ui: (key: UiKey | GalleryKey, slots?: Slots) =>
      isGallery(key) ? gallery(w.lang, key, slots) : w.ui(key, slots),
    f: (key: FrameKey | GalleryKey, slots?: Slots) =>
      isGallery(key) ? gallery(w.lang, key, slots) : w.f(key, slots),
  };
}
