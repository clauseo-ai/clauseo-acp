// The Hindi of words-mcp-tools.ts, keyed by its English, provisional (a Hindi-speaking lawyer to review). Loaded only
// for a Hindi display: words-hi.ts gathers every words file's Hindi into one lazy chunk, and words-mcp-tools.ts imports
// this file's type alone.

export const HI = {
  // UC-169: the section's name
  Tools: "टूल",
  // UC-170, UC-171: the plain line at the top, then the Safe workspace's line under it
  "A tool runs on this computer. It can reach anything your account here can: your files, your programs and the internet.":
    "टूल इसी कंप्यूटर पर चलता है। यहाँ आपका खाता जिस भी चीज़ तक पहुँच सकता है, टूल भी पहुँच सकता है: आपकी फ़ाइलें, आपके प्रोग्राम और इंटरनेट।",
  "Not used in the Safe workspace.": "सुरक्षित कार्यक्षेत्र में इस्तेमाल नहीं होता।",
  // UC-172, UC-173: the empty list, and the button that adds one
  "No tools yet.": "अभी कोई टूल नहीं।",
  "Add a tool": "टूल जोड़ें",
  // UC-174, UC-175: the firm's states; UC-175 also when a save meets the firm's lock
  "Your firm set these tools.": "ये टूल आपकी फ़र्म ने तय किए हैं।",
  "Your firm manages tools on this computer.": "इस कंप्यूटर पर टूल आपकी फ़र्म संभालती है।",
  // UC-176 to UC-179: the form's fields; {example} is a full path as filed
  Name: "नाम",
  Command: "कमांड",
  "Use the program's full path, such as {example}.": "प्रोग्राम का पूरा पाथ लिखें, जैसे {example}।",
  "Arguments, one per line": "आर्ग्युमेंट, हर पंक्ति में एक",
  // UC-180 to UC-182: the environment values; a saved value is never shown, only that it is saved
  "Environment values": "एनवायरनमेंट वैल्यू",
  Saved: "सहेजा गया",
  Replace: "बदलें",
  // UC-183: the confirm before Remove; {name} is the tool's name as filed
  "Remove {name}?": "{name} हटाएँ?",
  // UC-184 to UC-186: what the host refuses, said under its field
  "Give the tool a name.": "टूल का नाम लिखें।",
  "That name can't be used. Choose another.": "यह नाम इस्तेमाल नहीं हो सकता। कोई दूसरा चुनें।",
  "Use letters, numbers, - and _ only.": "केवल अक्षर, अंक, - और _ इस्तेमाल करें।",
  // UC-187 to UC-189: a tool at an address (behind REMOTE_TOOLS until remote tools land); {example} is an address
  "Use a full address, such as {example}.": "पूरा पता लिखें, जैसे {example}।",
  Address: "पता",
  Headers: "हेडर",
} as const;
