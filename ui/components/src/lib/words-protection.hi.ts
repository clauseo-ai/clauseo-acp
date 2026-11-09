// The Hindi of words-protection.ts, keyed by its English. Loaded only for a Hindi display
// (words-hi.ts). The waiting lines are the design's own, with its Hindi; every other line is new
// (UC), its Hindi provisional until a Hindi-speaking lawyer reviews it. Every line that names the
// thing says Safe workspace (सुरक्षित कार्यक्षेत्र; Rohan's design B).

export const HI = {
  // the waiting lines, the design's (UC-14, UC-15): here, not in words-first-launch.ts, so the room's protection chunk
  // and the first run's chunk share no words chunk of their own
  "Choose Yes in the window Windows shows.": "Windows जो विंडो दिखाता है, उसमें Yes चुनें।",
  "Setting up the Safe workspace.": "सुरक्षित कार्यक्षेत्र तैयार हो रहा है।",
  // the check's own waiting line (R17 PR-B's in_progress "check"), provisional
  "Checking the Safe workspace.": "सुरक्षित कार्यक्षेत्र की जाँच हो रही है।",
  // the set-up's confirm (UC-11, UC-40), where it brings Windows' prompt: design B's installer does
  // the first set-up, and the app's elevates the verified admin copy after an in-app Remove, or for
  // a re-run
  "Set up the Safe workspace?": "सुरक्षित कार्यक्षेत्र तैयार करें?",
  "Windows will ask to let Clauseo set up the Safe workspace. It will say the publisher is unknown, because this early version of Clauseo isn't signed yet. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.":
    "Windows, Clauseo को सुरक्षित कार्यक्षेत्र तैयार करने देने के लिए पूछेगा। वह प्रकाशक को अज्ञात बताएगा, क्योंकि Clauseo के इस शुरुआती संस्करण पर अभी डिजिटल हस्ताक्षर नहीं है। Yes चुनें। अगर Windows किसी व्यवस्थापक का पासवर्ड माँगे, तो इस कंप्यूटर की देखरेख करने वाले व्यक्ति उसे दर्ज कर सकते हैं।",
  // Add room's confirm (UC-94, UC-95)
  "Add room": "जगह जोड़ें",
  "Add room for 4 more?": "4 और के लिए जगह जोड़ें?",
  "Windows will ask to let Clauseo add room to the Safe workspace. It will say the publisher is unknown, because this early version of Clauseo isn't signed yet. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.":
    "Windows, Clauseo को सुरक्षित कार्यक्षेत्र में जगह जोड़ने देने के लिए पूछेगा। वह प्रकाशक को अज्ञात बताएगा, क्योंकि Clauseo के इस शुरुआती संस्करण पर अभी डिजिटल हस्ताक्षर नहीं है। Yes चुनें। अगर Windows किसी व्यवस्थापक का पासवर्ड माँगे, तो इस कंप्यूटर की देखरेख करने वाले व्यक्ति उसे दर्ज कर सकते हैं।",
  // the set-up press where no Windows prompt follows (UC-12)
  "Set up the Safe workspace": "सुरक्षित कार्यक्षेत्र तैयार करें",
  // the remove confirm (UC-41 to UC-44)
  "Remove the Safe workspace from this computer?": "इस कंप्यूटर से सुरक्षित कार्यक्षेत्र हटाएँ?",
  "Windows will ask to let Clauseo remove the Safe workspace. It will say the publisher is unknown, because this early version of Clauseo isn't signed yet. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.":
    "Windows, Clauseo को सुरक्षित कार्यक्षेत्र हटाने देने के लिए पूछेगा। वह प्रकाशक को अज्ञात बताएगा, क्योंकि Clauseo के इस शुरुआती संस्करण पर अभी डिजिटल हस्ताक्षर नहीं है। Yes चुनें। अगर Windows किसी व्यवस्थापक का पासवर्ड माँगे, तो इस कंप्यूटर की देखरेख करने वाले व्यक्ति उसे दर्ज कर सकते हैं।",
  "Afterwards, Clauseo works on this computer without the Safe workspace until it's set up again.":
    "इसके बाद, जब तक सुरक्षित कार्यक्षेत्र फिर से तैयार नहीं होता, Clauseo इस कंप्यूटर पर उसके बिना काम करता है।",
  "Remove set-up": "सेटअप हटाएँ",
  // the notes (UC-22, UC-23, UC-25)
  "The Safe workspace isn't set up on this computer yet. Until it is, Clauseo can read any file you can open on this computer, and asks before changing anything outside this matter's folder.":
    "इस कंप्यूटर पर सुरक्षित कार्यक्षेत्र अभी तैयार नहीं है। तब तक Clauseo इस कंप्यूटर की हर वह फ़ाइल पढ़ सकता है जिसे आप खोल सकते हैं, और इस मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "The Safe workspace couldn't be set up on this computer, so Clauseo can read any file you can open on it. It asks before changing anything outside this matter's folder.":
    "इस कंप्यूटर पर सुरक्षित कार्यक्षेत्र तैयार नहीं हो सका, इसलिए Clauseo इसकी हर वह फ़ाइल पढ़ सकता है जिसे आप खोल सकते हैं। वह इस मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "This computer can't run the Safe workspace, so Clauseo can read any file you can open on it. It asks before changing anything outside this matter's folder.":
    "यह कंप्यूटर सुरक्षित कार्यक्षेत्र नहीं चला सकता, इसलिए Clauseo इसकी हर वह फ़ाइल पढ़ सकता है जिसे आप खोल सकते हैं। वह इस मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  // a build without protection (R17 PR-B's unsupported { why: "not_built" }, every real build until desktop wires
  // protection in), said calmly (UC-38)
  "This version of Clauseo doesn't have the Safe workspace yet. It comes in an update. Until then, Clauseo can read any file you can open on this computer, and asks before changing anything outside this matter's folder.":
    "Clauseo के इस संस्करण में अभी सुरक्षित कार्यक्षेत्र नहीं है। यह एक अपडेट में आएगा। तब तक Clauseo इस कंप्यूटर की हर वह फ़ाइल पढ़ सकता है जिसे आप खोल सकते हैं, और इस मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  // the pool, counts only (UC-27 to UC-32, UC-34, UC-35)
  "All {size} places in this computer's Safe workspace are in use, so this matter works without it for now. It goes back in once a place is free.":
    "इस कंप्यूटर के सुरक्षित कार्यक्षेत्र की सभी {size} जगहें उपयोग में हैं, इसलिए यह मामला अभी उसके बिना काम करता है। कोई जगह खाली होते ही यह फिर उसमें आ जाएगा।",
  "In use by other open matters: {n}": "खुले दूसरे मामलों के उपयोग में: {n}",
  "Kept by matters with nothing open: {n}": "ऐसे मामलों के पास जिनमें कुछ खुला नहीं: {n}",
  "Freed the next time Clauseo starts work: {n}": "Clauseo के अगली बार काम शुरू करने पर खाली होंगे: {n}",
  "Still holding files set aside: {n}": "अलग रखी फ़ाइलें अब भी इनमें हैं: {n}",
  "To free one now, end a conversation in another matter.":
    "अभी एक स्थान खाली करने के लिए किसी दूसरे मामले की बातचीत समाप्त करें।",
  "Clauseo can add room for 4 more. Windows asks for permission first.":
    "Clauseo 4 और स्थानों के लिए जगह जोड़ सकता है। Windows पहले अनुमति माँगता है।",
  // Windows error 1385, once R17 PR-E names it (UC-33)
  "Windows isn't letting the Safe workspace's accounts sign in. Setting up the Safe workspace again usually fixes this. If it doesn't, ask whoever looks after this computer to add ClauseoSandboxUsers to the Allow log on locally policy.":
    "Windows सुरक्षित कार्यक्षेत्र के खातों को साइन इन नहीं करने दे रहा है। सुरक्षित कार्यक्षेत्र को फिर से तैयार करने से आमतौर पर यह ठीक हो जाता है। अगर ऐसा न हो, तो इस कंप्यूटर की देखरेख करने वाले व्यक्ति से ClauseoSandboxUsers को Allow log on locally नीति में जोड़ने को कहें।",
  // the firm (UC-50, UC-51)
  "Your firm requires the Safe workspace on this computer. Set it up once, then ask here.":
    "आपकी फ़र्म इस कंप्यूटर पर सुरक्षित कार्यक्षेत्र ज़रूरी मानती है। इसे एक बार तैयार करें, फिर यहाँ पूछें।",
  "Your firm requires the Safe workspace, and all its places on this computer are in use, so Clauseo can't answer here yet. End a conversation in another matter to free one.":
    "आपकी फ़र्म सुरक्षित कार्यक्षेत्र ज़रूरी मानती है, और इस कंप्यूटर पर उसकी सभी जगहें उपयोग में हैं, इसलिए Clauseo अभी यहाँ उत्तर नहीं दे सकता। एक जगह खाली करने के लिए किसी दूसरे मामले में बातचीत समाप्त करें।",
  "Your firm requires the Safe workspace, and this version of Clauseo doesn't have it yet, so Clauseo can't answer here. The Safe workspace comes in an update.":
    "आपकी फ़र्म सुरक्षित कार्यक्षेत्र ज़रूरी मानती है, और Clauseo के इस संस्करण में यह अभी नहीं है, इसलिए Clauseo यहाँ उत्तर नहीं दे सकता। सुरक्षित कार्यक्षेत्र एक अपडेट में आएगा।",
  // design B: what stops the Safe workspace (R17 PR-E's NotProtectedCause blocked { reason } and
  // other_account), said by blocked.ts; the body, in the note and in Settings, then the reason's
  // line (UC-80 to UC-95)
  "Something on this computer stops the Safe workspace, so Clauseo can read any file you can open on it. It asks before changing anything outside this matter's folder.":
    "इस कंप्यूटर पर कोई चीज़ सुरक्षित कार्यक्षेत्र को रोक रही है, इसलिए Clauseo इसकी हर वह फ़ाइल पढ़ सकता है जिसे आप खोल सकते हैं। वह इस मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "Something on this computer stops the Safe workspace, so Clauseo asks before changing anything outside a matter's folder.":
    "इस कंप्यूटर पर कोई चीज़ सुरक्षित कार्यक्षेत्र को रोक रही है, इसलिए Clauseo किसी मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "A Windows policy on this computer blocks the program that sets up the Safe workspace. Ask your IT team to allow it.":
    "इस कंप्यूटर की एक Windows नीति सुरक्षित कार्यक्षेत्र तैयार करने वाले प्रोग्राम को रोकती है। अपनी IT टीम से इसकी अनुमति देने को कहें।",
  "The program that sets up the Safe workspace is missing, or security software blocked it. Ask your IT team to look into it.":
    "सुरक्षित कार्यक्षेत्र तैयार करने वाला प्रोग्राम नहीं मिला, या किसी सुरक्षा सॉफ़्टवेयर ने उसे रोक दिया। अपनी IT टीम से इसकी जाँच करने को कहें।",
  "The Safe workspace wasn't set up when Clauseo was installed.":
    "Clauseo इंस्टॉल होते समय सुरक्षित कार्यक्षेत्र तैयार नहीं हुआ।",
  "The program that sets up the Safe workspace has changed since Clauseo was installed. Ask your IT team, or install Clauseo again.":
    "Clauseo इंस्टॉल होने के बाद से सुरक्षित कार्यक्षेत्र तैयार करने वाला प्रोग्राम बदल गया है। अपनी IT टीम से पूछें, या Clauseo फिर से इंस्टॉल करें।",
  "The folder that holds the Safe workspace's set-up program isn't limited to administrators, or holds files it shouldn't. Ask your IT team to look into it.":
    "सुरक्षित कार्यक्षेत्र का सेटअप प्रोग्राम जिस फ़ोल्डर में है, वह केवल व्यवस्थापकों तक सीमित नहीं है, या उसमें ऐसी फ़ाइलें हैं जो नहीं होनी चाहिए। अपनी IT टीम से इसकी जाँच करने को कहें।",
  "The Safe workspace on this computer belongs to another Windows account. Ask your IT team.":
    "इस कंप्यूटर का सुरक्षित कार्यक्षेत्र किसी दूसरे Windows खाते का है। अपनी IT टीम से पूछें।",
  "Ask your IT team to look into it.": "अपनी IT टीम से इसकी जाँच करने को कहें।",
  // the repair, where the installer's set-up didn't happen: a personal install's line and its
  // button (which opens the installed version's release page, installer.ts), and a firm install's
  // line
  "Run the Clauseo installer again to set up the Safe workspace.":
    "सुरक्षित कार्यक्षेत्र तैयार करने के लिए Clauseo इंस्टॉलर फिर से चलाएँ।",
  "Get the installer": "इंस्टॉलर पाएँ",
  // the close line beside the button: the button quits Clauseo once the page opened (installer.ts),
  // provisional
  "Clauseo will close so the installer can set up the Safe workspace. Run the installer you download, then open Clauseo again.":
    "Clauseo बंद हो जाएगा, ताकि इंस्टॉलर सुरक्षित कार्यक्षेत्र सेट अप कर सके। जो इंस्टॉलर आप डाउनलोड करें, उसे चलाएँ, फिर Clauseo दोबारा खोलें।",
  "Ask your IT team to install Clauseo again.": "अपनी IT टीम से Clauseo फिर से इंस्टॉल करने को कहें।",
  "Your firm requires the Safe workspace on this computer, so Clauseo can't answer here until it's set up.":
    "आपकी फ़र्म इस कंप्यूटर पर सुरक्षित कार्यक्षेत्र ज़रूरी मानती है, इसलिए जब तक यह तैयार नहीं होता, Clauseo यहाँ उत्तर नहीं दे सकता।",
  // sandbox.blocked, a command's refusal (not in ErrorCode on this base; #639 adds it)
  "Something on this computer stops the Safe workspace, so Clauseo can't answer here. Ask your IT team to look into it.":
    "इस कंप्यूटर पर कोई चीज़ सुरक्षित कार्यक्षेत्र को रोक रही है, इसलिए Clauseo यहाँ उत्तर नहीं दे सकता। अपनी IT टीम से इसकी जाँच करने को कहें।",
  // #766's data_fence_names_only: Settings' Safe workspace and the open note, while the data folder's
  // identity can't be read (provisional)
  "Clauseo can't confirm its own data folder on this computer right now, so it protects that folder by checking its name only. This clears by itself once Clauseo can confirm the folder again.":
    "Clauseo अभी इस कंप्यूटर पर अपना डेटा फ़ोल्डर पक्का नहीं कर पा रहा, इसलिए वह उस फ़ोल्डर को सिर्फ़ उसके नाम की जाँच से सुरक्षित रखता है। जब Clauseo फ़ोल्डर को फिर से पक्का कर पाएगा, यह अपने-आप हट जाएगा।",
} as const;
