// The Hindi of words-folder.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-folder.ts imports this file's type alone.

export const HI = {
  // FR1: the notice's title, then its tooltip and accessible name
  "Clauseo asks before each edit": "Clauseo हर संपादन से पहले पूछता है",
  "Clauseo asks before each edit in this folder": "Clauseo इस फ़ोल्डर में हर संपादन से पहले पूछता है",
  // FR2: behind FR1's info icon
  "You can change files here, but not this folder's permissions. Clauseo needs them to keep Claude and Codex inside the matter. Some edits won't go through.":
    "आप यहाँ फ़ाइलें बदल सकते हैं, पर इस फ़ोल्डर की अनुमतियाँ नहीं। Claude और Codex को मामले के भीतर रखने के लिए Clauseo को इनकी ज़रूरत है। कुछ संपादन नहीं हो पाएँगे।",
  // FR3: the notice's button, which opens FR5
  "What to tell IT": "IT को क्या बताएँ",
  // FR5: the text to send IT, selected, with Copy; {path} is the matter's folder
  'Please give my account "Change permissions" on this folder: {path}. Clauseo, the app I use for this matter, adds one entry to the folder\'s permissions so Claude and Codex, which it runs, can write only inside it.':
    'कृपया इस फ़ोल्डर पर मेरे खाते को "Change permissions" दें: {path}। इस मामले के लिए मेरा ऐप Clauseo फ़ोल्डर की अनुमतियों में एक प्रविष्टि जोड़ता है, ताकि वह जिन Claude और Codex को चलाता है, वे केवल इसी फ़ोल्डर के भीतर लिख सकें।',
  Copy: "कॉपी करें",
  // FR12: a network drive's notice: its words, its tooltip and accessible name, and behind its info icon
  "On a network drive.": "नेटवर्क ड्राइव पर।",
  "Claude asks before every change. Codex can't work here.":
    "Claude हर बदलाव से पहले पूछता है। Codex यहाँ काम नहीं कर सकता।",
  "This matter's folder is on a network drive": "इस मामले का फ़ोल्डर नेटवर्क ड्राइव पर है",
  "Clauseo can't keep Claude and Codex inside this folder. For full protection, move the matter to a folder on this computer.":
    "Clauseo, Claude और Codex को इस फ़ोल्डर के भीतर नहीं रख सकता। पूरे संरक्षण के लिए मामले को इस कंप्यूटर के किसी फ़ोल्डर में ले जाएँ।",
  // FR15: a drive without file permissions, as FR12
  "No file permissions.": "फ़ाइल अनुमतियाँ नहीं।",
  "This matter's folder is on a drive that doesn't keep file permissions":
    "इस मामले का फ़ोल्डर ऐसी ड्राइव पर है जो फ़ाइल अनुमतियाँ नहीं रखती",
  "Clauseo can't keep Claude and Codex inside this folder. USB drives often don't keep permissions. For full protection, move the matter to a folder on this computer.":
    "Clauseo, Claude और Codex को इस फ़ोल्डर के भीतर नहीं रख सकता। USB ड्राइव अक्सर अनुमतियाँ नहीं रखतीं। पूरे संरक्षण के लिए मामले को इस कंप्यूटर के किसी फ़ोल्डर में ले जाएँ।",
  // FR7: New matter's warning for a folder picked on a network drive, then its tooltip and accessible name
  "On a network drive": "नेटवर्क ड्राइव पर",
  "This folder is on a network drive": "यह फ़ोल्डर नेटवर्क ड्राइव पर है",
  // FR8: the warning's body is FR12's second line; behind its info icon (provisional: "an agent" reworded)
  "Clauseo can't keep Claude and Codex inside a folder on a network drive.":
    "Clauseo, Claude और Codex को नेटवर्क ड्राइव के फ़ोल्डर के भीतर नहीं रख सकता।",
  // FR9: the warning's main button, which opens the picker again
  "Choose another folder": "दूसरा फ़ोल्डर चुनें",
  // FR10: the warning's other button, which makes the matter there
  "Create anyway": "फिर भी बनाएँ",
  // FR13: the warning for a drive without file permissions, then its tooltip and accessible name
  "No file permissions": "फ़ाइल अनुमतियाँ नहीं",
  "This drive doesn't keep file permissions": "यह ड्राइव फ़ाइल अनुमतियाँ नहीं रखती",
  // FR14: its body as FR8's; behind its info icon (provisional: "an agent" reworded)
  "Clauseo can't keep Claude and Codex inside a folder on this drive. USB drives often don't keep permissions.":
    "Clauseo, Claude और Codex को इस ड्राइव के फ़ोल्डर के भीतर नहीं रख सकता। USB ड्राइव अक्सर अनुमतियाँ नहीं रखतीं।",
  // FR20 (provisional): the room's notice for a folder the host couldn't check
  "Clauseo couldn't check this folder, so Claude asks before every change.":
    "Clauseo इस फ़ोल्डर की जाँच नहीं कर सका, इसलिए Claude हर बदलाव से पहले पूछता है।",
} as const;
