// The Hindi of words-folder-card.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-folder-card.ts imports this file's type alone.

export const HI = {
  // FR6: the reason under "Editing files" on a permission card, and behind its info icon
  "Can't set folder permissions": "फ़ोल्डर की अनुमतियाँ सेट नहीं हो सकतीं",
  "Clauseo can't change this folder's permissions, so it asks before each edit.":
    "Clauseo इस फ़ोल्डर की अनुमतियाँ नहीं बदल सकता, इसलिए हर संपादन से पहले पूछता है।",
} as const;
