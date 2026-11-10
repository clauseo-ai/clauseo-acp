// The Hindi of words-toolbar.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-toolbar.ts imports this file's type alone. words-versions.hi.ts spreads
// it into Versions' list.

export const HI = {
  Versions: "संस्करण",
  "Not protected": "संरक्षित नहीं",
  "Not protected.": "संरक्षित नहीं।",
  // UC: the room's Not protected line by plan; UC-03 is the design's (Said By Its Cause), with its Hindi
  "Clauseo changes files in this matter without asking, and asks before changing files outside it.":
    "Clauseo इस मामले की फ़ाइलें बिना पूछे बदलता है, और इसके बाहर की फ़ाइलें बदलने से पहले पूछता है।",
  // UC-04: ChatGPT's, which reads any file the lawyer can open and asks only before changing files outside the matter
  "Clauseo can read any file you can open, and changes files in this matter without asking. It asks before changing files outside it.":
    "Clauseo हर वह फ़ाइल पढ़ सकता है जिसे आप खोल सकते हैं, और इस मामले की फ़ाइलें बिना पूछे बदलता है। इसके बाहर की फ़ाइलें बदलने से पहले यह पूछता है।",
} as const;
