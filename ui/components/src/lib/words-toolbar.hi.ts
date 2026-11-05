// The Hindi of words-toolbar.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-toolbar.ts imports this file's type alone. words-versions.hi.ts spreads
// it into Versions' list.

export const HI = {
  Versions: "संस्करण",
  "Not protected": "संरक्षित नहीं",
  "Not protected.": "संरक्षित नहीं।",
  "Clauseo can reach beyond this matter's folder without asking.":
    "Clauseo बिना पूछे इस मामले के फ़ोल्डर से बाहर पहुँच सकता है।",
} as const;
