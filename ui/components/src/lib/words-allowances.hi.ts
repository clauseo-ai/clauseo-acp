// The Hindi of words-allowances.ts, keyed by its English. Loaded only for a Hindi display:
// words-hi.ts gathers every words file's Hindi into one lazy chunk, and words-allowances.ts imports
// this file's type alone. Provisional (app track): the Safe workspace is सुरक्षित कार्यक्षेत्र
// (words-protection.hi.ts), a matter मामला, an answer kept याद रखा गया उत्तर, allowed अनुमति
// (words-blocklist.hi.ts), and Cancel रद्द करें, as words-ui.hi.ts has it.

export const HI = {
  // Settings: the section's name, its lead, the empty list and the line under the list
  "Remembered answers": "याद रखे गए उत्तर",
  "The folders and sites each matter keeps your answer for.":
    "वे फ़ोल्डर और वेबसाइटें जिनके लिए हर मामला आपका उत्तर याद रखता है।",
  "No remembered answers. When you allow or refuse a folder or site for a matter and keep the answer, it shows here.":
    "कोई याद रखा गया उत्तर नहीं है। जब आप किसी मामले के लिए किसी फ़ोल्डर या वेबसाइट को अनुमति देते हैं या मना करते हैं और उत्तर याद रखते हैं, तो वह यहाँ दिखता है।",
  "Forgetting a folder takes effect the next time Clauseo starts work in that matter.":
    "किसी फ़ोल्डर को भुलाने का असर अगली बार होता है जब Clauseo उस मामले में काम शुरू करता है।",
  // a group whose matter this window doesn't know
  "A matter not on your list": "एक मामला जो आपकी सूची में नहीं है",
  // a row: what it covers, and the answer
  "Folder, read only": "फ़ोल्डर, केवल पढ़ने के लिए",
  "File, read only": "फ़ाइल, केवल पढ़ने के लिए",
  Folder: "फ़ोल्डर",
  File: "फ़ाइल",
  Site: "वेबसाइट",
  "From another matter, read only": "दूसरे मामले से, केवल पढ़ने के लिए",
  "From another matter": "दूसरे मामले से",
  "From a newer version of Clauseo": "Clauseo के नए संस्करण से",
  Allowed: "अनुमति है",
  "Not allowed": "अनुमति नहीं है",
  Forget: "भुला दें",
  // {target} isolated: a folder, a file or a site
  "Forget {target}": "{target} को भुला दें",
  // the room: the protection note's list, and its switched-off line's button and line under it
  "Remembered for this matter": "इस मामले के लिए याद रखा गया",
  "Switch it back on": "इसे फिर से चालू करें",
  "It applies the next time Clauseo starts work in this matter.":
    "यह अगली बार लागू होता है जब Clauseo इस मामले में काम शुरू करता है।",
  // the matter's menu
  "Switch off the Safe workspace…": "सुरक्षित कार्यक्षेत्र बंद करें…",
  "Switch the Safe workspace back on": "सुरक्षित कार्यक्षेत्र फिर से चालू करें",
  "Use files from another matter…": "दूसरे मामले की फ़ाइलें इस्तेमाल करें…",
  // opens Settings at its Remembered answers section, for a matter that has any
  "Remembered answers…": "याद रखे गए उत्तर…",
  // a switch-off refused with sandbox.required, on the matter row's note line
  "Your firm requires the Safe workspace in every matter, so it can't be switched off.":
    "आपकी फ़र्म हर मामले में सुरक्षित कार्यक्षेत्र ज़रूरी मानती है, इसलिए इसे बंद नहीं किया जा सकता।",
  // the list of the other matters
  "Use files from another matter": "दूसरे मामले की फ़ाइलें इस्तेमाल करें",
  "Choose the matter. Clauseo then opens its folder so you can pick the files, which it can only read.":
    "मामला चुनें। फिर Clauseo उसका फ़ोल्डर खोलता है ताकि आप फ़ाइलें चुन सकें, जिन्हें वह केवल पढ़ सकता है।",
  Cancel: "रद्द करें",
} as const;
