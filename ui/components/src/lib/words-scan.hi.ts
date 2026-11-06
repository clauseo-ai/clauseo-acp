// The Hindi of words-scan.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-scan.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // UD2-01, UD2-02: the counts so far
  "{n} files, {mb} MB": "{n} फ़ाइलें, {mb} MB",
  "1 file, {mb} MB": "1 फ़ाइल, {mb} MB",
  // UD2-03, UD2-04: the file the walk reached, or none yet
  "Reading the folder. Now {title}.": "फ़ोल्डर पढ़ा जा रहा है। अभी {title}।",
  "Reading the folder.": "फ़ोल्डर पढ़ा जा रहा है।",
  // UD2-06, UD2-07: a folder a cloud service syncs
  "This folder syncs with {service}, so changes to its papers sync too, including Clauseo's. Versions are kept on this computer, outside the folder.":
    "यह फ़ोल्डर {service} से सिंक होता है, इसलिए इसके दस्तावेज़ों के बदलाव भी सिंक होते हैं, Clauseo के बदलाव भी। संस्करण इसी कंप्यूटर पर, फ़ोल्डर के बाहर रखे जाते हैं।",
  "Got it": "ठीक है",
  // UD2-08 to UD2-11: files kept only online
  "{n} files are kept only in {service} and download as they're read.":
    "{n} फ़ाइलें केवल {service} में रखी हैं और पढ़ते समय डाउनलोड होती हैं।",
  "1 file is kept only in {service} and downloads as it's read.":
    "1 फ़ाइल केवल {service} में रखी है और पढ़ते समय डाउनलोड होती है।",
  "{n} files are kept only online and download as they're read.":
    "{n} फ़ाइलें केवल ऑनलाइन रखी हैं और पढ़ते समय डाउनलोड होती हैं।",
  "1 file is kept only online and downloads as it's read.":
    "1 फ़ाइल केवल ऑनलाइन रखी है और पढ़ते समय डाउनलोड होती है।",
} satisfies Hindi<string>;
