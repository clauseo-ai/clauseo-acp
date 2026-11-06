// The Hindi of words-errors.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-errors.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  "Something went wrong inside Clauseo.": "Clauseo के भीतर कुछ गड़बड़ हो गई।",
  "Clauseo couldn't take that request as it was.": "Clauseo वह अनुरोध उस रूप में नहीं ले सका।",
  "That request clashes with one already made, so nothing changed.":
    "यह अनुरोध पहले किए गए अनुरोध से टकराता है, इसलिए कुछ नहीं बदला।",
  "This matter is no longer in Clauseo.": "यह मामला अब Clauseo में नहीं है।",
  "This conversation is no longer in Clauseo.": "यह बातचीत अब Clauseo में नहीं है।",
  "Wait for the answer to finish, or stop it.": "उत्तर पूरा होने तक प्रतीक्षा करें, या उसे रोकें।",
  "The answer to that question has already started or finished.":
    "उस प्रश्न का उत्तर पहले ही शुरू या पूरा हो चुका है।",
  "Too many questions are waiting in this conversation.": "इस बातचीत में बहुत अधिक प्रश्न प्रतीक्षा में हैं।",
  "That request was already answered, or it has expired.":
    "उस अनुरोध का उत्तर पहले ही दिया जा चुका है, या उसका समय निकल गया है।",
  "That answer wasn't one of the choices offered.": "वह उत्तर दिए गए विकल्पों में से नहीं था।",
  "That question was already answered, or it has expired.":
    "उस प्रश्न का उत्तर पहले ही दिया जा चुका है, या उसका समय निकल गया है।",
  "Those answers don't fit the question.": "ये उत्तर प्रश्न से मेल नहीं खाते।",
  "Claude didn't start. Your question is kept.": "Claude शुरू नहीं हुआ। आपका प्रश्न रखा गया है।",
  "Claude sent something Clauseo can't read.": "Claude ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
  "Sign in to Claude first. Your question is kept.":
    "पहले Claude में साइन इन करें। आपका प्रश्न रखा गया है।",
  "This version of Claude Code doesn't work with Clauseo.":
    "Claude Code का यह संस्करण Clauseo के साथ काम नहीं करता।",
  "Clauseo can't start Claude safely here. Reinstall Clauseo.":
    "Clauseo यहाँ Claude को सुरक्षित रूप से शुरू नहीं कर सकता। Clauseo फिर से इंस्टॉल करें।",
  // the agent.* sentences above for a thread on the ChatGPT plan (Codex), for an agent of no kind by its name, and
  // with no name at all (PLANNED). The lawyer signs in to Codex outside Clauseo until stage D's sign-in, so
  // auth_required says where, with no button.
  "ChatGPT didn't start. Your question is kept.": "ChatGPT शुरू नहीं हुआ। आपका प्रश्न रखा गया है।",
  "{name} didn't start. Your question is kept.": "{name} शुरू नहीं हुआ। आपका प्रश्न रखा गया है।",
  "The AI didn't start. Your question is kept.": "AI शुरू नहीं हुआ। आपका प्रश्न रखा गया है।",
  "ChatGPT sent something Clauseo can't read.": "ChatGPT ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
  "{name} sent something Clauseo can't read.": "{name} ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
  "The AI sent something Clauseo can't read.": "AI ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
  "Sign in to ChatGPT in Codex first. Your question is kept.":
    "पहले Codex में ChatGPT से साइन इन करें। आपका प्रश्न रखा गया है।",
  "Sign in to {name} first. Your question is kept.":
    "पहले {name} में साइन इन करें। आपका प्रश्न रखा गया है।",
  "Sign in to the AI first. Your question is kept.": "पहले AI में साइन इन करें। आपका प्रश्न रखा गया है।",
  "This version of Codex doesn't work with Clauseo.":
    "Codex का यह संस्करण Clauseo के साथ काम नहीं करता।",
  "This version of {name} doesn't work with Clauseo.":
    "{name} का यह संस्करण Clauseo के साथ काम नहीं करता।",
  "This version of the AI doesn't work with Clauseo.": "AI का यह संस्करण Clauseo के साथ काम नहीं करता।",
  "Clauseo can't start ChatGPT safely here. Reinstall Clauseo.":
    "Clauseo यहाँ ChatGPT को सुरक्षित रूप से शुरू नहीं कर सकता। Clauseo फिर से इंस्टॉल करें।",
  "Clauseo can't start {name} safely here. Reinstall Clauseo.":
    "Clauseo यहाँ {name} को सुरक्षित रूप से शुरू नहीं कर सकता। Clauseo फिर से इंस्टॉल करें।",
  "Clauseo can't start the AI safely here. Reinstall Clauseo.":
    "Clauseo यहाँ AI को सुरक्षित रूप से शुरू नहीं कर सकता। Clauseo फिर से इंस्टॉल करें।",
  "The computer's disk is full. Free some space, then try again.":
    "कंप्यूटर की डिस्क भर गई है। कुछ जगह खाली करें, फिर दोबारा प्रयास करें।",
  "Clauseo couldn't read back its saved data.": "Clauseo अपना सहेजा गया डेटा वापस नहीं पढ़ सका।",
  "Clauseo couldn't read or write its data. Try again.":
    "Clauseo अपना डेटा पढ़ या लिख नहीं सका। फिर से प्रयास करें।",
  "Clauseo is already open in another window.": "Clauseo पहले से किसी दूसरी विंडो में खुला है।",
  "This data comes from a newer version of Clauseo.": "यह डेटा Clauseo के नए संस्करण से है।",
  "Clauseo couldn't save a version of this matter's papers, so this change can't be undone.":
    "Clauseo इस मामले के दस्तावेज़ों का संस्करण नहीं सहेज सका, इसलिए यह बदलाव पूर्ववत नहीं किया जा सकता।",
  "This matter's folder can't be found. It may have moved, or its drive isn't connected.":
    "इस मामले का फ़ोल्डर नहीं मिल रहा। हो सकता है वह कहीं और ले जाया गया हो, या उसकी ड्राइव जुड़ी न हो।",
  "A whole drive, your home folder or a system folder can't be a matter.":
    "पूरी ड्राइव, आपका होम फ़ोल्डर या कोई सिस्टम फ़ोल्डर मामला नहीं बन सकता।",
  "This matter's folder holds a .codex folder. Rename or remove it to use Codex here.":
    "इस मामले के फ़ोल्डर में एक .codex फ़ोल्डर है। यहाँ Codex का उपयोग करने के लिए उसका नाम बदलें या उसे हटाएँ।",
  "A paper is open in another program. Close it, then try again.":
    "कोई दस्तावेज़ किसी दूसरे प्रोग्राम में खुला है। उसे बंद करें, फिर दोबारा प्रयास करें।",
  "Wait for the answer to finish first.": "पहले उत्तर पूरा होने दें।",
  "A whole drive can't be a matter.": "पूरी ड्राइव मामला नहीं बन सकती।",
  "Your home folder can't be a matter.": "आपका होम फ़ोल्डर मामला नहीं बन सकता।",
  "A system folder can't be a matter.": "कोई सिस्टम फ़ोल्डर मामला नहीं बन सकता।",
  "Clauseo can't read that folder.": "Clauseo वह फ़ोल्डर नहीं पढ़ पा रहा।",
  "That folder is already a matter's, or inside one.":
    "वह फ़ोल्डर पहले से किसी मामले का है, या किसी मामले के भीतर है।",
  "That folder holds Clauseo's own data.": "उस फ़ोल्डर में Clauseo का अपना डेटा है।",
  "That wasn't allowed, so nothing changed.": "इसकी अनुमति नहीं थी, इसलिए कुछ नहीं बदला।",
  "That is too large to send.": "यह भेजने के लिए बहुत बड़ा है।",
  "Your organisation manages that setting, so it can't be changed.":
    "यह सेटिंग आपका संगठन संभालता है, इसलिए इसे बदला नहीं जा सकता।",
  "Clauseo couldn't save that setting. Your settings are as they were.":
    "Clauseo वह सेटिंग सहेज नहीं सका। आपकी सेटिंग्स पहले जैसी हैं।",
  "A key is already kept on this computer. To change it, open Settings, then Plans.":
    "इस कंप्यूटर पर पहले से एक key रखी है। उसे बदलने के लिए सेटिंग्स खोलें, फिर प्लान।",
  "Clauseo is using the ChatGPT login already on this computer. To change it, choose 'Stop using it here', then sign in.":
    "Clauseo इस कंप्यूटर पर पहले से मौजूद ChatGPT लॉगिन इस्तेमाल कर रहा है। इसे बदलने के लिए 'यहाँ इस्तेमाल बंद करें' चुनें, फिर साइन इन करें।",
  "Questions go through your firm's own account, so there's nothing to sign in to here.":
    "प्रश्न आपकी फ़र्म के अपने खाते से जाते हैं, इसलिए यहाँ साइन इन करने को कुछ नहीं है।",
  "Clauseo is using your API key. To sign in with your plan instead, open Settings, then Plans, and remove the key.":
    "Clauseo आपकी API key इस्तेमाल कर रहा है। इसके बजाय अपने प्लान से साइन इन करने के लिए सेटिंग्स खोलें, फिर प्लान, और key हटाएँ।",
  "Clauseo is using your firm's address. To sign in with your plan instead, open Settings, then Plans, and remove the address.":
    "Clauseo आपकी फ़र्म का पता इस्तेमाल कर रहा है। इसके बजाय अपने प्लान से साइन इन करने के लिए सेटिंग्स खोलें, फिर प्लान, और पता हटाएँ।",
  "You're offline. Setting up waits for the connection.":
    "आप ऑफ़लाइन हैं। कनेक्शन मिलने तक सेटअप रुका रहेगा।",
  "Your network is blocking the download.": "आपका नेटवर्क डाउनलोड रोक रहा है।",
  "There isn't enough free space on this computer for the download.":
    "इस कंप्यूटर पर डाउनलोड के लिए पर्याप्त खाली जगह नहीं है।",
  "Setting up didn't finish. Try again.": "सेटअप पूरा नहीं हुआ। फिर से प्रयास करें।",
  "Sign-in was cancelled in the browser.": "ब्राउज़र में साइन इन रद्द कर दिया गया।",
  "The sign-in link expired. Try again.": "साइन इन का लिंक समाप्त हो गया। फिर से प्रयास करें।",
  "This account has no plan that works with Clauseo.":
    "इस खाते में ऐसा कोई प्लान नहीं है जो Clauseo के साथ काम करे।",
  "Sign-in with a code is turned off for this ChatGPT account.":
    "इस ChatGPT खाते के लिए कोड से साइन इन बंद है।",
  "Another program on this computer is using ChatGPT's sign-in.":
    "इस कंप्यूटर पर कोई दूसरा प्रोग्राम ChatGPT का साइन इन इस्तेमाल कर रहा है।",
  "Sign-in didn't finish. Try again.": "साइन इन पूरा नहीं हुआ। फिर से प्रयास करें।",
  "Clauseo can't read your firm's sign-in settings, so it won't sign in until they can be read.":
    "Clauseo आपकी फ़र्म की साइन इन सेटिंग्स नहीं पढ़ पा रहा, इसलिए उनके पढ़े जाने तक साइन इन नहीं करेगा।",
  "Codex can't work on a network drive. Move the matter to this computer.":
    "Codex नेटवर्क ड्राइव पर काम नहीं कर सकता। मामले को इस कंप्यूटर पर ले जाएँ।",
  "Codex can't work on a drive without file permissions. Move the matter to this computer.":
    "Codex बिना फ़ाइल अनुमतियों वाली ड्राइव पर काम नहीं कर सकता। मामले को इस कंप्यूटर पर ले जाएँ।",
  "This matter's folder is too deep inside other folders for Clauseo to work in it on Windows. Move it nearer the top of its drive.":
    "इस मामले का फ़ोल्डर दूसरे फ़ोल्डरों के बहुत भीतर है, इसलिए Windows पर Clauseo उसमें काम नहीं कर सकता। उसे ड्राइव के ऊपरी स्तर के पास ले जाएँ।",
  "The update didn't install. Try again.": "अपडेट इंस्टॉल नहीं हुआ। फिर से प्रयास करें।",
  "Clauseo is still finishing. Try again in a moment.":
    "Clauseo अभी काम पूरा कर रहा है। थोड़ी देर में फिर से प्रयास करें।",
  "Clauseo is still starting. Try again in a moment.":
    "Clauseo अभी शुरू हो रहा है। थोड़ी देर में फिर से प्रयास करें।",
  "Clauseo couldn't check for a saved API key in time. If this computer asks for permission, allow it, then try again.":
    "Clauseo समय पर सहेजी गई API key की जाँच नहीं कर सका। अगर यह कंप्यूटर अनुमति माँगे, तो अनुमति दें, फिर दोबारा प्रयास करें।",
  "This file's path is too long for Word. Move the matter's folder nearer the top of the drive, or shorten its folder names.":
    "इस फ़ाइल का पाथ Word के लिए बहुत लंबा है। मामले के फ़ोल्डर को ड्राइव के ऊपरी स्तर के पास ले जाएँ, या उसके फ़ोल्डरों के नाम छोटे करें।",
  "A file in this matter's folder is a shortcut. Clauseo can't protect a shortcut, so it won't answer in this matter until it's gone. Move or delete it, then ask again.":
    "इस मामले के फ़ोल्डर में एक फ़ाइल शॉर्टकट है। Clauseo शॉर्टकट को संरक्षित नहीं कर सकता, इसलिए उसके हटने तक इस मामले में उत्तर नहीं देगा। उसे कहीं और ले जाएँ या हटाएँ, फिर दोबारा पूछें।",
  "Protection wasn't set up, because Windows didn't get permission. Try again, and choose Yes when Windows asks.":
    "संरक्षण सेट अप नहीं हुआ, क्योंकि Windows को अनुमति नहीं मिली। फिर से प्रयास करें, और Windows के पूछने पर Yes चुनें।",
  "Protection couldn't be set up on this computer. Try again. If it keeps failing, ask whoever looks after this computer to look into it.":
    "इस कंप्यूटर पर संरक्षण सेट अप नहीं हो सका। फिर से प्रयास करें। बार-बार विफल हो, तो इस कंप्यूटर की देखरेख करने वाले व्यक्ति से इसे दिखवाएँ।",
  "Clauseo needs to set up protection on this computer once before it can answer here. Set it up, then ask again. Your question is kept.":
    "यहाँ उत्तर देने से पहले Clauseo को इस कंप्यूटर पर एक बार संरक्षण सेट अप करना होगा। इसे सेट अप करें, फिर दोबारा पूछें। आपका प्रश्न रखा गया है।",
  "Your firm requires protection, and this computer can't provide it, so Clauseo can't answer here. Ask whoever looks after this computer.":
    "आपकी फ़र्म संरक्षण अनिवार्य करती है, और यह कंप्यूटर इसे नहीं दे सकता, इसलिए Clauseo यहाँ उत्तर नहीं दे सकता। इस कंप्यूटर की देखरेख करने वाले व्यक्ति से पूछें।",
  "Clauseo can't use the Codex sign-in already on this computer, because Clauseo's data folder is on a different drive from your home folder. Sign in to Codex in Clauseo instead.":
    "Clauseo इस कंप्यूटर पर पहले से मौजूद Codex साइन इन का उपयोग नहीं कर सकता, क्योंकि Clauseo का डेटा फ़ोल्डर आपके होम फ़ोल्डर से अलग ड्राइव पर है। इसके बजाय Clauseo में Codex में साइन इन करें।",
} as const;

/** The sentences' Hindi, for the copy review to tell hi: later from Hindi. */
export const errorHindi: Hindi<keyof typeof HI> = HI;
