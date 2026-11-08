// The Hindi of words-toolbar.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-toolbar.ts imports this file's type alone. words-versions.hi.ts spreads
// it into Versions' list.

export const HI = {
  Versions: "संस्करण",
  "Not protected": "संरक्षित नहीं",
  "Not protected.": "संरक्षित नहीं।",
  "Clauseo can reach beyond this matter's folder without asking.":
    "Clauseo बिना पूछे इस मामले के फ़ोल्डर से बाहर पहुँच सकता है।",
  // UC: the room's Not protected line by plan; UC-03 is the design's (Said By Its Cause), with its Hindi
  "Clauseo asks before each change in this matter.": "Clauseo इस मामले में हर बदलाव से पहले पूछता है।",
  // new (QC.6's approved English)
  "Clauseo can change files in this matter without asking. It asks before going beyond it.":
    "Clauseo इस मामले में बिना पूछे फ़ाइलें बदल सकता है। इससे बाहर जाने से पहले यह पूछता है।",
} as const;
