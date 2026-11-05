// The Hindi of words-startup.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-startup.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  "Clauseo couldn't open. A newer version of Clauseo saved its data, so this version can't read it. Your papers stay in their folders.":
    "Clauseo नहीं खुल सका। इसका डेटा Clauseo के नए संस्करण ने सहेजा है, इसलिए यह संस्करण उसे पढ़ नहीं सकता। आपके दस्तावेज़ अपने फ़ोल्डरों में ही हैं।",
  "Clauseo couldn't open. Its saved data doesn't read back. Your papers stay in their folders.":
    "Clauseo नहीं खुल सका। इसका सहेजा गया डेटा वापस नहीं पढ़ा जा रहा। आपके दस्तावेज़ अपने फ़ोल्डरों में ही हैं।",
  "Clauseo couldn't open. The computer's disk is full. Free some space, then open Clauseo again.":
    "Clauseo नहीं खुल सका। कंप्यूटर की डिस्क भर गई है। कुछ जगह खाली करें, फिर Clauseo दोबारा खोलें।",
  "Clauseo couldn't open. It couldn't read or write its data. Open Clauseo again.":
    "Clauseo नहीं खुल सका। यह अपना डेटा पढ़ या लिख नहीं सका। Clauseo दोबारा खोलें।",
} as const;

/** Every key, for the banned-words screen in the tests. */
export const startupWordList: Hindi<keyof typeof HI> = HI;
