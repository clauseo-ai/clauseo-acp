// The Hindi of words-ui.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-ui.ts imports this file's type alone.

export const HI = {
  // a matter's actions, from its row in the list
  "Actions for {name}": "{name} के विकल्प",
  "Pin to the top": "सबसे ऊपर पिन करें",
  "Show in Finder": "Finder में दिखाएँ",
  "Show in File Explorer": "File Explorer में दिखाएँ",
  "Show in folder": "फ़ोल्डर में दिखाएँ",
  "Remove from the list": "सूची से हटाएँ",
  "The folder and its papers stay where they are.": "फ़ोल्डर और उसके दस्तावेज़ अपनी जगह पर रहते हैं।",
  // the plan to ask with
  "Plan to ask with": "प्रश्न किस प्लान से पूछें",
  // restoring a version
  Restore: "पुनर्स्थापित करें",
  Cancel: "रद्द करें",
  // undoing over a later edit
  "Undo anyway puts back the version from before Clauseo's edit. The changes made since stay in Versions.":
    "फिर भी पूर्ववत करने पर Clauseo के संपादन से पहले का संस्करण लौट आता है। उसके बाद के बदलाव संस्करणों में बने रहते हैं।",
  "Undo anyway": "फिर भी पूर्ववत करें",
  "Keep current": "मौजूदा संस्करण रखें",
  // settings, in sections
  Settings: "सेटिंग्स",
  Matters: "मामले",
  // a build with the e2e feature, at the rail's foot and in Settings' About
  "Test build": "टेस्ट बिल्ड",
  // lite, in settings: until the host reports remote desktops, On and Off only
  "Lite mode": "सादा मोड",
  "Takes away every shadow and all motion.": "सभी छायाएँ और सारी हलचल हटा देता है।",
  On: "चालू",
  "Always, on this computer": "इस कंप्यूटर पर हमेशा",
  Off: "बंद",
  "Never, even over a remote desktop": "कभी नहीं, रिमोट डेस्कटॉप पर भी नहीं",
  // a confirmation
  Close: "बंद करें",
  Notifications: "सूचनाएँ",
  // the matter's folder and its note
  "Locate folder": "फ़ोल्डर ढूँढें",
  // the computer's own dialog, open over a prompt or the edge's ask
  "macOS is asking you to confirm.": "macOS आपसे पुष्टि माँग रहा है।",
  "Windows is asking you to confirm.": "Windows आपसे पुष्टि माँग रहा है।",
  "Linux is asking you to confirm.": "Linux आपसे पुष्टि माँग रहा है।",
} as const;
