// The Hindi of words-mcp-tools.ts, keyed by its English, provisional (a Hindi-speaking lawyer to review). Loaded only
// for a Hindi display: words-hi.ts gathers every words file's Hindi into one lazy chunk, and words-mcp-tools.ts imports
// this file's type alone.

export const HI = {
  // UC-169: the section's name
  Tools: "टूल",
  // UC-170, UC-171: the plain line at the top, then the Safe workspace's line under it
  "Tools run on this computer with the same access you have: your files, your programs and the internet. Add only tools you trust.":
    "टूल इसी कंप्यूटर पर उसी पहुँच के साथ चलते हैं जो आपके पास है: आपकी फ़ाइलें, आपके प्रोग्राम और इंटरनेट। केवल उन्हीं टूल को जोड़ें जिन पर आपको भरोसा है।",
  "Not used in the Safe workspace.": "सुरक्षित कार्यक्षेत्र में इस्तेमाल नहीं होता।",
  // UC-172, UC-173: the empty list, and the button that adds one
  "No tools yet.": "अभी कोई टूल नहीं।",
  "Add a tool": "टूल जोड़ें",
  // UC-174, UC-175: the firm's states; UC-175 also when a save meets the firm's lock
  "Your firm set these tools.": "ये टूल आपकी फ़र्म ने तय किए हैं।",
  "Your firm manages tools on this computer.": "इस कंप्यूटर पर टूल आपकी फ़र्म संभालती है।",
  // UC-176 to UC-179: the form's fields; UC-178 is the Command field's help
  Name: "नाम",
  Command: "कमांड",
  "A program name such as npx, or its full path.": "प्रोग्राम का नाम, जैसे npx, या उसका पूरा पाथ।",
  "Arguments, one per line": "आर्ग्युमेंट, हर पंक्ति में एक",
  // UC-180 to UC-182: the environment values; a saved value is never shown, only that it is saved
  "Environment values": "एनवायरनमेंट वैल्यू",
  Saved: "सहेजा गया",
  Replace: "बदलें",
  // UC-183: the confirm before Remove; {name} is the tool's name as filed
  "Remove {name}?": "{name} हटाएँ?",
  // UC-184 to UC-186: what the host refuses, said under its field (UC-186 for tool and header names)
  "Give the tool a name.": "टूल का नाम लिखें।",
  "That name can't be used. Choose another.": "यह नाम इस्तेमाल नहीं हो सकता। कोई दूसरा चुनें।",
  "Use letters, numbers, - and _ only.": "केवल अक्षर, अंक, - और _ इस्तेमाल करें।",
  // UC-187 to UC-189: a tool at an address (behind REMOTE_TOOLS until remote tools land); {example} is an address
  "Use a full address, such as {example}.": "पूरा पता लिखें, जैसे {example}।",
  Address: "पता",
  Headers: "हेडर",
  // UC-205: an environment value's name refused (envNameInvalid): no "-" in such a name
  "Use letters, numbers and _ only.": "केवल अक्षर, अंक और _ इस्तेमाल करें।",
  // UC-190 to UC-194: the Import buttons, one per app found ({source} is the app's name as it names itself:
  // Claude Desktop, Claude Code, Codex), and the one line after an import ({names} the servers left out, as filed)
  "Import {count} tools from {source}": "{source} से {count} टूल इम्पोर्ट करें",
  "Import 1 tool from {source}": "{source} से 1 टूल इम्पोर्ट करें",
  "Imported {count} tools.": "{count} टूल इम्पोर्ट हुए।",
  "Imported 1 tool.": "1 टूल इम्पोर्ट हुआ।",
  "{count} couldn't be imported: {names}.": "{count} इम्पोर्ट नहीं हो सके: {names}।",
  // UC-195, UC-196: the Command field's refusals; {program} is what the lawyer typed, as filed
  "Clauseo couldn't find {program} on this computer.": "Clauseo को इस कंप्यूटर पर {program} नहीं मिला।",
  "Use a program name or its full path.": "प्रोग्राम का नाम या उसका पूरा पाथ लिखें।",
} as const;
