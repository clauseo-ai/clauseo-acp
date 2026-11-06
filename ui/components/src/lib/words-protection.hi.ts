// The Hindi of words-protection.ts, keyed by its English. Loaded only for a Hindi display (words-hi.ts).
// The waiting lines are the design's own, with its Hindi; every other line is new (UC), its
// Hindi provisional until a Hindi-speaking lawyer reviews it.

export const HI = {
  // the waiting lines, the design's (UC-14, UC-15): here, not in words-first-launch.ts, so the room's protection chunk
  // and the first run's chunk share no words chunk of their own
  "Choose Yes in the window Windows shows.": "Windows जो विंडो दिखाता है, उसमें Yes चुनें।",
  "Setting up the Safe workspace.": "सुरक्षित कार्यक्षेत्र तैयार हो रहा है।",
  // the set-up confirm (UC-11, UC-12, UC-40)
  "Set up protection?": "संरक्षण सेट अप करें?",
  "Windows will ask to let Clauseo set up its protection. It will say the publisher is unknown, because this early version of Clauseo isn't signed yet. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.":
    "Windows, Clauseo को उसका संरक्षण सेट अप करने देने के लिए पूछेगा। वह प्रकाशक को अज्ञात बताएगा, क्योंकि Clauseo के इस शुरुआती संस्करण पर अभी डिजिटल हस्ताक्षर नहीं है। Yes चुनें। अगर Windows किसी व्यवस्थापक का पासवर्ड माँगे, तो इस कंप्यूटर की देखरेख करने वाले व्यक्ति उसे दर्ज कर सकते हैं।",
  "Set up protection": "संरक्षण सेट अप करें",
  // the remove confirm (UC-41 to UC-44)
  "Remove protection's set-up?": "संरक्षण का सेटअप हटाएँ?",
  "Windows will ask to let Clauseo remove its protection's set-up. It will say the publisher is unknown, because this early version of Clauseo isn't signed yet. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.":
    "Windows, Clauseo को उसके संरक्षण का सेटअप हटाने देने के लिए पूछेगा। वह प्रकाशक को अज्ञात बताएगा, क्योंकि Clauseo के इस शुरुआती संस्करण पर अभी डिजिटल हस्ताक्षर नहीं है। Yes चुनें। अगर Windows किसी व्यवस्थापक का पासवर्ड माँगे, तो इस कंप्यूटर की देखरेख करने वाले व्यक्ति उसे दर्ज कर सकते हैं।",
  "Afterwards, Clauseo works on this computer without protection until you set it up again.":
    "इसके बाद, जब तक आप इसे फिर से सेट अप नहीं करते, Clauseo इस कंप्यूटर पर बिना संरक्षण के काम करेगा।",
  "Remove set-up": "सेटअप हटाएँ",
  // the notes (UC-22, UC-23, UC-25)
  "Protection isn't set up on this computer yet. Until it is, Clauseo can read any file on this computer, and asks before changing anything outside this matter's folder.":
    "इस कंप्यूटर पर संरक्षण अभी सेट अप नहीं है। तब तक Clauseo इस कंप्यूटर की कोई भी फ़ाइल पढ़ सकता है, और इस मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "Protection couldn't be set up on this computer, so Clauseo can read any file on it. It asks before changing anything outside this matter's folder.":
    "इस कंप्यूटर पर संरक्षण सेट अप नहीं हो सका, इसलिए Clauseo इसकी कोई भी फ़ाइल पढ़ सकता है। यह इस मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "This computer can't give Clauseo's protection, so Clauseo can read any file on it. It asks before changing anything outside this matter's folder.":
    "यह कंप्यूटर Clauseo का संरक्षण नहीं दे सकता, इसलिए Clauseo इसकी कोई भी फ़ाइल पढ़ सकता है। यह इस मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  // a build without protection (R17 PR-B's unsupported { why: "not_built" }, every real build until desktop wires
  // protection in), said calmly (UC-38)
  "This build of Clauseo comes without protection, so Clauseo can read any file on this computer. It asks before changing anything outside this matter's folder.":
    "Clauseo का यह बिल्ड संरक्षण के बिना आता है, इसलिए Clauseo इस कंप्यूटर की कोई भी फ़ाइल पढ़ सकता है। यह इस मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  // the pool, counts only (UC-27 to UC-32, UC-34, UC-35)
  "All {size} of this computer's protected spaces are in use, so this matter works without protection for now. It's protected again once one is free.":
    "इस कंप्यूटर के सभी {size} संरक्षित स्थान उपयोग में हैं, इसलिए यह मामला अभी बिना संरक्षण के चलता है। एक स्थान खाली होते ही यह फिर से संरक्षित हो जाएगा।",
  "In use by other open matters: {n}": "खुले दूसरे मामलों के उपयोग में: {n}",
  "Kept by matters with nothing open: {n}": "ऐसे मामलों के पास जिनमें कुछ खुला नहीं: {n}",
  "Freed the next time Clauseo starts work: {n}": "Clauseo के अगली बार काम शुरू करने पर खाली होंगे: {n}",
  "Still holding files set aside: {n}": "अलग रखी फ़ाइलें अब भी इनमें हैं: {n}",
  "To free one now, end a conversation in another matter.":
    "अभी एक स्थान खाली करने के लिए किसी दूसरे मामले की बातचीत समाप्त करें।",
  "Clauseo can add room for 4 more. Windows asks for permission first.":
    "Clauseo 4 और स्थानों के लिए जगह जोड़ सकता है। Windows पहले अनुमति माँगता है।",
  "Add room": "जगह जोड़ें",
  // Windows error 1385, once R17 PR-E names it (UC-33)
  "Windows policy on this computer doesn't let Clauseo's protection accounts sign in. Ask your IT team to add them to the Allow log on locally policy.":
    "इस कंप्यूटर की Windows नीति Clauseo के संरक्षण खातों को साइन इन नहीं करने देती। अपनी IT टीम से कहें कि उन्हें Allow log on locally नीति में जोड़ दे।",
  // the firm (UC-50, UC-51)
  "Your firm requires protection on this computer. Set it up once, then ask here.":
    "आपकी फ़र्म इस कंप्यूटर पर संरक्षण अनिवार्य करती है। इसे एक बार सेट अप करें, फिर यहाँ पूछें।",
  "Your firm requires protection, and all of this computer's protected spaces are in use, so Clauseo can't answer here yet. End a conversation in another matter to free one.":
    "आपकी फ़र्म संरक्षण अनिवार्य करती है, और इस कंप्यूटर के सभी संरक्षित स्थान उपयोग में हैं, इसलिए Clauseo अभी यहाँ उत्तर नहीं दे सकता। एक स्थान खाली करने के लिए किसी दूसरे मामले की बातचीत समाप्त करें।",
} as const;
