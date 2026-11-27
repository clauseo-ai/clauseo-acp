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
  "Claude didn't start. Try again. Your question is kept.":
    "Claude शुरू नहीं हुआ। फिर से प्रयास करें। आपका प्रश्न रखा गया है।",
  "Claude sent something Clauseo can't read.": "Claude ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
  "Sign in with Claude first. Your question is kept.":
    "पहले Claude से साइन इन करें। आपका प्रश्न रखा गया है।",
  "This version of Claude Code doesn't work with Clauseo.":
    "Claude Code का यह संस्करण Clauseo के साथ काम नहीं करता।",
  "Clauseo can't start Claude safely here. Reinstall Clauseo.":
    "Clauseo यहाँ Claude को सुरक्षित रूप से शुरू नहीं कर सकता। Clauseo फिर से इंस्टॉल करें।",
  // the agent.* sentences above for a thread on the ChatGPT plan (Codex), for an agent of no kind by its name, and
  // with no name at all (PLANNED). The lawyer signs in to Codex outside Clauseo until stage D's sign-in, so
  // auth_required says where, with no button.
  "ChatGPT didn't start. Try again. Your question is kept.":
    "ChatGPT शुरू नहीं हुआ। फिर से प्रयास करें। आपका प्रश्न रखा गया है।",
  "{name} didn't start. Try again. Your question is kept.":
    "{name} शुरू नहीं हुआ। फिर से प्रयास करें। आपका प्रश्न रखा गया है।",
  "The AI didn't start. Try again. Your question is kept.":
    "AI शुरू नहीं हुआ। फिर से प्रयास करें। आपका प्रश्न रखा गया है।",
  "ChatGPT sent something Clauseo can't read.": "ChatGPT ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
  "{name} sent something Clauseo can't read.": "{name} ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
  "The AI sent something Clauseo can't read.": "AI ने कुछ ऐसा भेजा जिसे Clauseo पढ़ नहीं सकता।",
  // agent.auth_required while the plan runs on an API key: the key wasn't accepted, never a sign-in (UC-149, UD2-29's
  // words and Hindi)
  "{name} didn't accept the API key. Your question is kept.":
    "{name} ने API key स्वीकार नहीं की। आपका प्रश्न रखा गया है।",
  "Sign in with ChatGPT in Codex first. Your question is kept.":
    "पहले Codex में ChatGPT से साइन इन करें। आपका प्रश्न रखा गया है।",
  "Sign in with {name} first. Your question is kept.":
    "पहले {name} से साइन इन करें। आपका प्रश्न रखा गया है।",
  "Sign in first. Your question is kept.": "पहले साइन इन करें। आपका प्रश्न रखा गया है।",
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
  "This matter's folder can't be reached. It may have moved, or its drive or network isn't connected.":
    "इस मामले के फ़ोल्डर तक पहुँचा नहीं जा सकता। हो सकता है वह कहीं और ले जाया गया हो, या उसकी ड्राइव या नेटवर्क जुड़ा न हो।",
  "This folder doesn't hold this matter's Clauseo file. Locate the matter's folder to carry on.":
    "इस फ़ोल्डर में इस मामले की Clauseo फ़ाइल नहीं है। आगे बढ़ने के लिए मामले का फ़ोल्डर ढूँढें।",
  "Clauseo needs permission to open this folder.": "Clauseo को यह फ़ोल्डर खोलने की अनुमति चाहिए।",
  // provisional, as above: the Open Privacy settings button under it on the Mac, and its line (PRIVACY_KEYS)
  "Open Privacy settings": "गोपनीयता सेटिंग्स खोलें",
  "Turn on access for Clauseo, then ask again.": "Clauseo के लिए एक्सेस चालू करें, फिर दोबारा पूछें।",
  "A whole drive, your home folder or a system folder can't be a matter.":
    "पूरी ड्राइव, आपका होम फ़ोल्डर या कोई सिस्टम फ़ोल्डर मामला नहीं बन सकता।",
  "This matter's folder holds a .codex folder. Rename or remove it to use ChatGPT here.":
    "इस मामले के फ़ोल्डर में एक .codex फ़ोल्डर है। यहाँ ChatGPT का उपयोग करने के लिए उसका नाम बदलें या उसे हटाएँ।",
  "A paper is open in another program. Close it, then try again.":
    "कोई दस्तावेज़ किसी दूसरे प्रोग्राम में खुला है। उसे बंद करें, फिर दोबारा प्रयास करें।",
  // matter.turn_running, reworded 24 Nov (several answers at once). hi: later
  "Wait for the answers in this matter to finish.": null,
  "This is the top of a drive. Choose a folder on it.":
    "यह किसी ड्राइव का सबसे ऊपरी स्तर है। उस पर कोई फ़ोल्डर चुनें।",
  "This is your home folder. Choose a folder inside it.":
    "यह आपका होम फ़ोल्डर है। उसके भीतर कोई फ़ोल्डर चुनें।",
  "This is a system folder. Choose another folder.": "यह एक सिस्टम फ़ोल्डर है। दूसरा फ़ोल्डर चुनें।",
  "Clauseo can't read that folder.": "Clauseo वह फ़ोल्डर नहीं पढ़ पा रहा।",
  "Clauseo can't make a matter of this folder. Choose another folder.":
    "Clauseo इस फ़ोल्डर से मामला नहीं बना सकता। दूसरा फ़ोल्डर चुनें।",
  "That folder is where Clauseo keeps matters. Choose the folder for one matter.":
    "उस फ़ोल्डर में Clauseo मामले रखता है। एक मामले का फ़ोल्डर चुनें।",
  "This folder is part of Clauseo's own data. Choose another folder.":
    "यह फ़ोल्डर Clauseo के अपने डेटा का हिस्सा है। दूसरा फ़ोल्डर चुनें।",
  "That wasn't allowed, so nothing changed.": "इसकी अनुमति नहीं थी, इसलिए कुछ नहीं बदला।",
  "That is too large to send.": "यह भेजने के लिए बहुत बड़ा है।",
  "Your firm manages that setting, so it can't be changed.":
    "यह सेटिंग आपकी फ़र्म प्रबंधित करती है, इसलिए इसे बदला नहीं जा सकता।",
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
  "You're offline. Setting up waits for the connection.":
    "आप ऑफ़लाइन हैं। कनेक्शन मिलने तक सेटअप रुका रहेगा।",
  "Your network is blocking the download.": "आपका नेटवर्क डाउनलोड रोक रहा है।",
  "There isn't enough free space on this computer for the download.":
    "इस कंप्यूटर पर डाउनलोड के लिए पर्याप्त खाली जगह नहीं है।",
  "Setting up didn't finish. Try again.": "सेटअप पूरा नहीं हुआ। फिर से प्रयास करें।",
  "Sign-in was cancelled.": "साइन इन रद्द हो गया।",
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
  "ChatGPT can't work on a network drive. Move the matter to this computer.":
    "ChatGPT नेटवर्क ड्राइव पर काम नहीं कर सकता। मामले को इस कंप्यूटर पर ले जाएँ।",
  "ChatGPT can't work on a drive without file permissions. Move the matter to this computer.":
    "ChatGPT बिना फ़ाइल अनुमतियों वाली ड्राइव पर काम नहीं कर सकता। मामले को इस कंप्यूटर पर ले जाएँ।",
  "This matter's folder is too deep inside other folders for Clauseo to work in it on Windows. Move it nearer the top of its drive.":
    "इस मामले का फ़ोल्डर दूसरे फ़ोल्डरों के बहुत भीतर है, इसलिए Windows पर Clauseo उसमें काम नहीं कर सकता। उसे ड्राइव के ऊपरी स्तर के पास ले जाएँ।",
  "The update didn't install. Try again.": "अपडेट इंस्टॉल नहीं हुआ। फिर से प्रयास करें।",
  "Clauseo is still finishing. Try again in a moment.":
    "Clauseo अभी काम पूरा कर रहा है। थोड़ी देर में फिर से प्रयास करें।",
  "Clauseo is still starting. Try again in a moment.":
    "Clauseo अभी शुरू हो रहा है। थोड़ी देर में फिर से प्रयास करें।",
  "Clauseo couldn't check for a saved API key in time. If this computer asks for permission, allow it, then try again.":
    "Clauseo समय पर सहेजी गई API key की जाँच नहीं कर सका। अगर यह कंप्यूटर अनुमति माँगे, तो अनुमति दें, फिर दोबारा प्रयास करें।",
  // agent.key_denied: the keychain refused an API key's read, save or removal; the Mac's (U3-err-agent.key_denied) and
  // Windows' and Linux's (U3-err-agent.key_denied.windows_linux). Provisional
  "Your Mac's keychain didn't let Clauseo read, save or remove the API key. Press Allow on Your keychain, then choose Always Allow.":
    "आपके Mac के कीचेन ने Clauseo को API कुंजी पढ़ने, सहेजने या हटाने नहीं दिया। Your keychain पर Allow दबाएँ, फिर Always Allow चुनें।",
  "Clauseo couldn't read, save or remove the API key on this computer. Try again. If it keeps happening, show this to whoever looks after this computer.":
    "Clauseo इस कंप्यूटर पर API कुंजी पढ़, सहेज या हटा नहीं सका। फिर से प्रयास करें। अगर ऐसा होता रहे, तो इसे उस व्यक्ति को दिखाएँ जो इस कंप्यूटर की देखभाल करता है।",
  // connector.keychain_refused: the keychain refused Case Law's saved sign-in (U3-err-connector.keychain_refused, provisional)
  "Your Mac's keychain didn't let Clauseo use your saved case-law sign-in, so nothing changed. Press Allow on Your keychain, then choose Always Allow, and try again.":
    "आपके Mac के कीचेन ने Clauseo को निर्णय खोजने के लिए सहेजा गया साइन-इन उपयोग नहीं करने दिया, इसलिए कुछ नहीं बदला। Your keychain पर Allow दबाएँ, फिर Always Allow चुनें, और दोबारा प्रयास करें।",
  "This file's path is too long for Word. Move the matter's folder nearer the top of the drive, or shorten its folder names.":
    "इस फ़ाइल का पाथ Word के लिए बहुत लंबा है। मामले के फ़ोल्डर को ड्राइव के ऊपरी स्तर के पास ले जाएँ, या उसके फ़ोल्डरों के नाम छोटे करें।",
  "A file in this matter's folder is a shortcut. Clauseo can't protect a shortcut, so it won't answer in this matter until it's gone. Move or delete it, then ask again.":
    "इस मामले के फ़ोल्डर में एक फ़ाइल शॉर्टकट है। Clauseo शॉर्टकट को संरक्षित नहीं कर सकता, इसलिए उसके हटने तक इस मामले में उत्तर नहीं देगा। उसे कहीं और ले जाएँ या हटाएँ, फिर दोबारा पूछें।",
  "Windows didn't get permission for the change Clauseo asked it to approve, so nothing changed. Try again, and choose Yes when Windows asks.":
    "Clauseo ने Windows से जिस बदलाव की स्वीकृति माँगी थी, उसकी अनुमति नहीं मिली, इसलिए कुछ नहीं बदला। फिर से कोशिश करें, और Windows के पूछने पर Yes चुनें।",
  "The Safe workspace couldn't be set up on this computer. Try again.":
    "इस कंप्यूटर पर सुरक्षित कार्यक्षेत्र तैयार नहीं हो सका। फिर से कोशिश करें।",
  "Clauseo can't answer here until the Safe workspace is set up on this computer. Your question is kept.":
    "जब तक इस कंप्यूटर पर सुरक्षित कार्यक्षेत्र तैयार नहीं होता, Clauseo यहाँ उत्तर नहीं दे सकता। आपका प्रश्न रखा गया है।",
  "Your firm requires the Safe workspace, and this computer can't provide it, so Clauseo can't answer here.":
    "आपकी फ़र्म सुरक्षित कार्यक्षेत्र ज़रूरी मानती है, और यह कंप्यूटर इसे नहीं दे सकता, इसलिए Clauseo यहाँ उत्तर नहीं दे सकता।",
  "Clauseo can't use the Safe workspace on this computer: a Windows setting or security software is blocking it, so nothing was started. Settings › Safe workspace says what to do.":
    "Clauseo इस कंप्यूटर पर सुरक्षित कार्यक्षेत्र का इस्तेमाल नहीं कर सकता: कोई Windows सेटिंग या सुरक्षा सॉफ़्टवेयर इसे रोक रहा है, इसलिए कुछ भी शुरू नहीं हुआ। क्या करना है, यह सेटिंग्स › सुरक्षित कार्यक्षेत्र में बताया गया है।",
  "Your firm has turned off allowing sites on Clauseo's unsafe-site list, so this site stays blocked.":
    "आपकी फ़र्म ने Clauseo की असुरक्षित वेबसाइटों की सूची में से किसी वेबसाइट को अनुमति देना बंद कर रखा है, इसलिए यह वेबसाइट अवरुद्ध रहेगी।",
  "This notice has expired. Open the site again, then allow it from the new notice.":
    "यह सूचना अब पुरानी हो चुकी है। वेबसाइट फिर से खोलें, फिर नई सूचना से उसे अनुमति दें।",
  "This site is allowed until Clauseo closes. Clauseo couldn't save the change, so it will be blocked again next time Clauseo opens.":
    "Clauseo के बंद होने तक इस वेबसाइट को अनुमति है। Clauseo यह बदलाव सहेज नहीं सका, इसलिए अगली बार Clauseo खुलने पर यह फिर से अवरुद्ध होगी।",
  "This site is blocked again until Clauseo closes. Clauseo couldn't save the change, so it may be allowed again next time Clauseo opens.":
    "Clauseo के बंद होने तक यह वेबसाइट फिर से अवरुद्ध है। Clauseo यह बदलाव सहेज नहीं सका, इसलिए अगली बार Clauseo खुलने पर इसे फिर से अनुमति मिल सकती है।",
  "Clauseo is still tidying up the Safe workspace after it last closed. Try again in a moment. Your question is kept.":
    "Clauseo पिछली बार बंद होने के बाद अभी भी सुरक्षित कार्यक्षेत्र को व्यवस्थित कर रहा है। थोड़ी देर में फिर से प्रयास करें। आपका प्रश्न रखा गया है।",
  "Claude was downloaded, but setting it up didn't finish. Try again.":
    "Claude डाउनलोड हो गया, लेकिन उसका सेटअप पूरा नहीं हुआ। फिर से प्रयास करें।",
  "Codex was downloaded, but setting it up didn't finish. Try again.":
    "Codex डाउनलोड हो गया, लेकिन उसका सेटअप पूरा नहीं हुआ। फिर से प्रयास करें।",
  "{name} was downloaded, but setting it up didn't finish. Try again.":
    "{name} डाउनलोड हो गया, लेकिन उसका सेटअप पूरा नहीं हुआ। फिर से प्रयास करें।",
  "The AI was downloaded, but setting it up didn't finish. Try again.":
    "AI डाउनलोड हो गया, लेकिन उसका सेटअप पूरा नहीं हुआ। फिर से प्रयास करें।",
  // the app's own: an answer's link to a paper the matter's folder no longer holds (provisional Hindi)
  "This paper is no longer in the matter's folder.": "यह दस्तावेज़ अब मामले के फ़ोल्डर में नहीं है।",
  // a Word file's tracked changes (provisional Hindi)
  "Clauseo's Word tool is missing. Reinstall Clauseo to accept or reject changes.":
    "Clauseo का Word टूल मौजूद नहीं है। बदलाव स्वीकार या अस्वीकार करने के लिए Clauseo फिर से इंस्टॉल करें।",
  "This file changed after the list was made, so nothing was accepted or rejected. The list now shows its current changes.":
    "सूची बनने के बाद यह फ़ाइल बदल गई, इसलिए कुछ भी स्वीकार या अस्वीकार नहीं किया गया। सूची अब इसके मौजूदा बदलाव दिखाती है।",
  "Clauseo can't read the tracked changes in this file. It may be damaged, or it may not be a Word document.":
    "Clauseo इस फ़ाइल के ट्रैक किए गए बदलाव नहीं पढ़ सकता। हो सकता है कि यह क्षतिग्रस्त हो, या यह Word दस्तावेज़ न हो।",
  "Clauseo couldn't save your decision. The file is as it was.":
    "Clauseo आपका निर्णय सहेज नहीं सका। फ़ाइल पहले जैसी ही है।",
  // the connection dropped mid-answer (provisional Hindi)
  "You're offline, so the answer stopped. The answer so far is kept. Try again when you're back online.":
    "आप ऑफ़लाइन हैं, इसलिए उत्तर रुक गया। अब तक का उत्तर रखा गया है। ऑनलाइन होने पर फिर से प्रयास करें।",
  // prompt.too_long: a question over the host's cap on a prompt's text, kept in the field (provisional Hindi)
  "This question is too long to send. Save the text as a file in the matter's folder, then ask about it. Your words are kept.":
    "यह प्रश्न भेजने के लिए बहुत लंबा है। पाठ को मामले के फ़ोल्डर में एक फ़ाइल के रूप में सहेजें, फिर उसके बारे में पूछें। आपके शब्द रखे गए हैं।",
  // Cancel in the native confirm before an MCP server is saved (provisional Hindi)
  "You chose Cancel, so nothing was saved.": "आपने रद्द करें चुना, इसलिए कुछ भी सहेजा नहीं गया।",
  // agent.answer_failed: the agent answered the question with an error no other code reads (provisional Hindi)
  "Claude stopped with an error. Try again.": "Claude एक त्रुटि के साथ रुक गया। फिर से प्रयास करें।",
  "ChatGPT stopped with an error. Try again.": "ChatGPT एक त्रुटि के साथ रुक गया। फिर से प्रयास करें।",
  "{name} stopped with an error. Try again.": "{name} एक त्रुटि के साथ रुक गया। फिर से प्रयास करें।",
  "The AI stopped with an error. Try again.": "AI एक त्रुटि के साथ रुक गया। फिर से प्रयास करें।",
  // agent.setting_refused: the agent didn't accept the lawyer's mode or setting change (provisional Hindi)
  "Claude didn't accept that change. Try again.":
    "Claude ने वह बदलाव स्वीकार नहीं किया। फिर से प्रयास करें।",
  "ChatGPT didn't accept that change. Try again.":
    "ChatGPT ने वह बदलाव स्वीकार नहीं किया। फिर से प्रयास करें।",
  "{name} didn't accept that change. Try again.":
    "{name} ने वह बदलाव स्वीकार नहीं किया। फिर से प्रयास करें।",
  "The AI didn't accept that change. Try again.": "AI ने वह बदलाव स्वीकार नहीं किया। फिर से प्रयास करें।",
  // agent.start_offline: the start failed on its connection, confirmed by the host's probe (provisional Hindi)
  "You're offline, so Claude didn't start. Try again when you're back online. Your question is kept.":
    "आप ऑफ़लाइन हैं, इसलिए Claude शुरू नहीं हुआ। ऑनलाइन होने पर फिर से प्रयास करें। आपका प्रश्न रखा गया है।",
  "You're offline, so ChatGPT didn't start. Try again when you're back online. Your question is kept.":
    "आप ऑफ़लाइन हैं, इसलिए ChatGPT शुरू नहीं हुआ। ऑनलाइन होने पर फिर से प्रयास करें। आपका प्रश्न रखा गया है।",
  "You're offline, so {name} didn't start. Try again when you're back online. Your question is kept.":
    "आप ऑफ़लाइन हैं, इसलिए {name} शुरू नहीं हुआ। ऑनलाइन होने पर फिर से प्रयास करें। आपका प्रश्न रखा गया है।",
  "You're offline, so the AI didn't start. Try again when you're back online. Your question is kept.":
    "आप ऑफ़लाइन हैं, इसलिए AI शुरू नहीं हुआ। ऑनलाइन होने पर फिर से प्रयास करें। आपका प्रश्न रखा गया है।",
  // agent.service_error: the agent's service had a problem and it gave up on the turn (#1030). ChatGPT's approved
  // (orchestrator, 2026-11-12); Claude's, {name}'s and the AI's derived from it (provisional Hindi)
  "Claude had a problem on its side. Try again.": "Claude की ओर से कोई समस्या आई। फिर से कोशिश करें।",
  "ChatGPT had a problem on its side. Try again.": "ChatGPT की ओर से कोई समस्या आई। फिर से कोशिश करें।",
  "{name} had a problem on its side. Try again.": "{name} की ओर से कोई समस्या आई। फिर से कोशिश करें।",
  "The AI had a problem on its side. Try again.": "AI की ओर से कोई समस्या आई। फिर से कोशिश करें।",
  // agent.quiet: the agent sent nothing for 5 minutes and the host ended the turn (D-050). The English is approved
  // (orchestrator, 2026-11-13); this Hindi is provisional
  "Claude stopped answering. Try again.": "Claude ने उत्तर देना बंद कर दिया। फिर से प्रयास करें।",
  "ChatGPT stopped answering. Try again.": "ChatGPT ने उत्तर देना बंद कर दिया। फिर से प्रयास करें।",
  "{name} stopped answering. Try again.": "{name} ने उत्तर देना बंद कर दिया। फिर से प्रयास करें।",
  "The AI stopped answering. Try again.": "AI ने उत्तर देना बंद कर दिया। फिर से प्रयास करें।",
  // agent.unreachable: the agent couldn't get through to its service while the computer seemed online (offline
  // truth). The English and this Hindi are provisional
  "Clauseo couldn't reach Claude. The answer so far is kept. Try again. If this keeps happening, ask your firm's IT.":
    "Clauseo, Claude तक नहीं पहुँच सका। अब तक का उत्तर रखा गया है। फिर से कोशिश करें। अगर ऐसा बार-बार हो, तो अपनी फ़र्म की IT टीम से पूछें।",
  "Clauseo couldn't reach ChatGPT. The answer so far is kept. Try again. If this keeps happening, ask your firm's IT.":
    "Clauseo, ChatGPT तक नहीं पहुँच सका। अब तक का उत्तर रखा गया है। फिर से कोशिश करें। अगर ऐसा बार-बार हो, तो अपनी फ़र्म की IT टीम से पूछें।",
  "Clauseo couldn't reach {name}. The answer so far is kept. Try again. If this keeps happening, ask your firm's IT.":
    "Clauseo, {name} तक नहीं पहुँच सका। अब तक का उत्तर रखा गया है। फिर से कोशिश करें। अगर ऐसा बार-बार हो, तो अपनी फ़र्म की IT टीम से पूछें।",
  "Clauseo couldn't reach the AI. The answer so far is kept. Try again. If this keeps happening, ask your firm's IT.":
    "Clauseo, AI तक नहीं पहुँच सका। अब तक का उत्तर रखा गया है। फिर से कोशिश करें। अगर ऐसा बार-बार हो, तो अपनी फ़र्म की IT टीम से पूछें।",
  // an MCP import too long to show whole in one native confirm (provisional Hindi)
  "There are too many tools to import at once, so nothing was imported. Add them one at a time.":
    "एक साथ आयात करने के लिए टूल बहुत अधिक हैं, इसलिए कुछ भी आयात नहीं हुआ। इन्हें एक-एक करके जोड़ें।",
  // matter.folder_backslash: the matter folder's path has a backslash, where the agent can't start. Draft English
  // (Rohan to approve); this Hindi is provisional
  "Claude can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.":
    "Claude इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। Finder में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
  "ChatGPT can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.":
    "ChatGPT इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। Finder में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
  "{name} can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.":
    "{name} इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। Finder में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
  "The AI can't work in this matter's folder because its name has a backslash (\\). Rename the folder in Finder, then Try again.":
    "AI इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। Finder में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
  // matter.folder_backslash on Linux, whose file manager has no one name. Draft English (Rohan to approve); this Hindi
  // is provisional
  "Claude can't work in this matter's folder because its name has a backslash (\\). Rename the folder in your file manager, then Try again.":
    "Claude इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। अपने फ़ाइल मैनेजर में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
  "ChatGPT can't work in this matter's folder because its name has a backslash (\\). Rename the folder in your file manager, then Try again.":
    "ChatGPT इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। अपने फ़ाइल मैनेजर में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
  "{name} can't work in this matter's folder because its name has a backslash (\\). Rename the folder in your file manager, then Try again.":
    "{name} इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। अपने फ़ाइल मैनेजर में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
  "The AI can't work in this matter's folder because its name has a backslash (\\). Rename the folder in your file manager, then Try again.":
    "AI इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके नाम में बैकस्लैश (\\) है। अपने फ़ाइल मैनेजर में फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।",
  // matter.folder_backslash when the backslash is in a folder above the matter's, named by {folder}: Finder on the
  // Mac, your file manager on Linux. Draft English (Rohan to approve); this Hindi is provisional
  "Claude can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in Finder, then Try again.":
    'Claude इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके पाथ में मौजूद फ़ोल्डर "{folder}" के नाम में बैकस्लैश (\\) है। Finder में उस फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।',
  "ChatGPT can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in Finder, then Try again.":
    'ChatGPT इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके पाथ में मौजूद फ़ोल्डर "{folder}" के नाम में बैकस्लैश (\\) है। Finder में उस फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।',
  "{name} can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in Finder, then Try again.":
    '{name} इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके पाथ में मौजूद फ़ोल्डर "{folder}" के नाम में बैकस्लैश (\\) है। Finder में उस फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।',
  "The AI can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in Finder, then Try again.":
    'AI इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके पाथ में मौजूद फ़ोल्डर "{folder}" के नाम में बैकस्लैश (\\) है। Finder में उस फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।',
  "Claude can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in your file manager, then Try again.":
    'Claude इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके पाथ में मौजूद फ़ोल्डर "{folder}" के नाम में बैकस्लैश (\\) है। अपने फ़ाइल मैनेजर में उस फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।',
  "ChatGPT can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in your file manager, then Try again.":
    'ChatGPT इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके पाथ में मौजूद फ़ोल्डर "{folder}" के नाम में बैकस्लैश (\\) है। अपने फ़ाइल मैनेजर में उस फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।',
  "{name} can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in your file manager, then Try again.":
    '{name} इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके पाथ में मौजूद फ़ोल्डर "{folder}" के नाम में बैकस्लैश (\\) है। अपने फ़ाइल मैनेजर में उस फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।',
  "The AI can't work in this matter's folder because the folder \"{folder}\" in its path has a backslash (\\). Rename that folder in your file manager, then Try again.":
    'AI इस मामले के फ़ोल्डर में काम नहीं कर सकता, क्योंकि उसके पाथ में मौजूद फ़ोल्डर "{folder}" के नाम में बैकस्लैश (\\) है। अपने फ़ाइल मैनेजर में उस फ़ोल्डर का नाम बदलें, उसके बाद फिर से प्रयास करें।',
  // agent.folder_inaccessible: the agent couldn't open the matter folder as it started. Draft English (Rohan to
  // approve); this Hindi is provisional
  "Claude couldn't open the matter's folder. Check that it's still there and hasn't been renamed, then Try again.":
    "Claude मामले का फ़ोल्डर नहीं खोल सका। जाँचें कि वह अब भी वहीं है और उसका नाम नहीं बदला गया है, उसके बाद फिर से प्रयास करें।",
  "ChatGPT couldn't open the matter's folder. Check that it's still there and hasn't been renamed, then Try again.":
    "ChatGPT मामले का फ़ोल्डर नहीं खोल सका। जाँचें कि वह अब भी वहीं है और उसका नाम नहीं बदला गया है, उसके बाद फिर से प्रयास करें।",
  "{name} couldn't open the matter's folder. Check that it's still there and hasn't been renamed, then Try again.":
    "{name} मामले का फ़ोल्डर नहीं खोल सका। जाँचें कि वह अब भी वहीं है और उसका नाम नहीं बदला गया है, उसके बाद फिर से प्रयास करें।",
  "The AI couldn't open the matter's folder. Check that it's still there and hasn't been renamed, then Try again.":
    "AI मामले का फ़ोल्डर नहीं खोल सका। जाँचें कि वह अब भी वहीं है और उसका नाम नहीं बदला गया है, उसके बाद फिर से प्रयास करें।",
  // word.too_large: a file over a size limit, in place of its tracked changes (provisional Hindi)
  "This file is too large for Clauseo to show its tracked changes.":
    "यह फ़ाइल इतनी बड़ी है कि Clauseo इसके ट्रैक किए गए बदलाव नहीं दिखा सकता।",
  // provisional (U3-err-agent.program_missing and .program_blocked, their agents' and .mac lines): Hindi later
  "A program Claude needs is missing from this computer, so Claude didn't start. Quit Clauseo and open it again, then choose Install when Clauseo offers it. Your question is kept.":
    null,
  "A program ChatGPT needs is missing from this computer, so ChatGPT didn't start. Quit Clauseo and open it again, then choose Install when Clauseo offers it. Your question is kept.":
    null,
  "A program {name} needs is missing from this computer, so {name} didn't start. Quit Clauseo and open it again, then choose Install when Clauseo offers it. Your question is kept.":
    null,
  "A program the AI needs is missing from this computer, so the AI didn't start. Quit Clauseo and open it again, then choose Install when Clauseo offers it. Your question is kept.":
    null,
  "This computer stopped a program Claude needs from starting, so Claude didn't start. If your antivirus blocked it, open Windows Security, choose Protection history, and allow it, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.":
    null,
  "This computer stopped a program ChatGPT needs from starting, so ChatGPT didn't start. If your antivirus blocked it, open Windows Security, choose Protection history, and allow it, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.":
    null,
  "This computer stopped a program {name} needs from starting, so {name} didn't start. If your antivirus blocked it, open Windows Security, choose Protection history, and allow it, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.":
    null,
  "This computer stopped a program the AI needs from starting, so the AI didn't start. If your antivirus blocked it, open Windows Security, choose Protection history, and allow it, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.":
    null,
  "This computer stopped a program Claude needs from starting, so Claude didn't start. If your antivirus blocked it, allow it there, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.":
    null,
  "This computer stopped a program ChatGPT needs from starting, so ChatGPT didn't start. If your antivirus blocked it, allow it there, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.":
    null,
  "This computer stopped a program {name} needs from starting, so {name} didn't start. If your antivirus blocked it, allow it there, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.":
    null,
  "This computer stopped a program the AI needs from starting, so the AI didn't start. If your antivirus blocked it, allow it there, then try again. Otherwise, whoever looks after this computer can allow it. Your question is kept.":
    null,
  "Clauseo can't restore this paper: a folder now has its name, or the folder it was in is now a file. Nothing has changed. Rename or move that folder or file, then try again.":
    "Clauseo यह दस्तावेज़ पुनर्स्थापित नहीं कर सकता: अब एक फ़ोल्डर का यही नाम है, या जिस फ़ोल्डर में यह था वह अब एक फ़ाइल है। कुछ नहीं बदला है। उस फ़ोल्डर या फ़ाइल का नाम बदलें या उसे कहीं और ले जाएँ, फिर दोबारा प्रयास करें।",
  // agent.home_failed: the agent's own folder not prepared. App-track English; hi: later, so each ships in English
  "Claude didn't start, because Clauseo couldn't set up the folder it keeps for Claude on this computer. Your question is kept. If Try again doesn't help, quit Clauseo and open it again. If it keeps happening, show this to whoever looks after this computer.":
    null,
  "ChatGPT didn't start, because Clauseo couldn't set up the folder it keeps for ChatGPT on this computer. Your question is kept. If Try again doesn't help, quit Clauseo and open it again. If it keeps happening, show this to whoever looks after this computer.":
    null,
  "{name} didn't start, because Clauseo couldn't set up the folder it keeps for {name} on this computer. Your question is kept. If Try again doesn't help, quit Clauseo and open it again. If it keeps happening, show this to whoever looks after this computer.":
    null,
  "The AI didn't start, because Clauseo couldn't set up the folder it keeps for the AI on this computer. Your question is kept. If Try again doesn't help, quit Clauseo and open it again. If it keeps happening, show this to whoever looks after this computer.":
    null,
  // matter.file_refused: the folder refused a change to a file (words-errors-refused.ts), by how much else the act did
  // and why. Draft English (Rohan to approve); the Hindi is to come (hi: later)
  "Clauseo couldn't change {name} because the file is set so it can't be changed. It's as it was.":
    null,
  "Clauseo couldn't change {name} because the same file also appears somewhere else on this computer. It's as it was.":
    null,
  "Clauseo left {name} as it was because it couldn't first check its saved copy of the file. Try again.":
    null,
  "Clauseo couldn't change {name} because the file is set so it can't be changed. Everything else was done.":
    null,
  "Clauseo couldn't change {name} because the same file also appears somewhere else on this computer. Everything else was done.":
    null,
  "Clauseo left {name} as it was because it couldn't first check its saved copy of the file. Everything else was done; try again to change this one too.":
    null,
  "Clauseo couldn't change {count} papers, including {name}, which is set so it can't be changed. Everything else was done.":
    null,
  "Clauseo couldn't change {count} papers, including {name}, which also appears somewhere else on this computer. Everything else was done.":
    null,
  "Clauseo left {count} papers as they were, including {name}, because it couldn't first check its saved copies. Everything else was done; try again to change them too.":
    null,
  // matter.file_refused, several files refused for different reasons (REFUSED_MIXED). The app track's English
  // (provisional); the Hindi is to come (hi: later)
  "Clauseo couldn't change {count} papers, including {name}, for different reasons. Everything else was done.":
    null,
  // matter.file_write_failed: writing a file in place stopped partway (words-errors-written.ts), by whether the file
  // was put back and whether other papers changed. The app track's English (provisional); the Hindi is to come
  // (hi: later)
  "Clauseo couldn't finish changing {name}, so it put the file back as it was. Nothing else was changed.":
    null,
  "Clauseo couldn't finish changing {name}, so it put that file back as it was. It may already have changed other papers. To put them back, restore the version from just before this in Versions.":
    null,
  "Clauseo couldn't finish changing {name}, and the file may be damaged. To put it right, restore the version from just before this in Versions.":
    null,
  // thread.background_work, prompt.handoff_too_long: the agent switch (issue 4a). Hindi provisional
  "Your message wasn't sent, because {name} is still finishing work it started in the background. Your message and the agent you picked are kept. Wait for it to finish, then send again.":
    "आपका संदेश नहीं भेजा गया, क्योंकि {name} पृष्ठभूमि में शुरू किया हुआ काम अभी पूरा कर रहा है। आपका संदेश और आपका चुना हुआ एजेंट सहेजे गए हैं। उसके पूरा होने तक रुकें, फिर दोबारा भेजें।",
  "Your message wasn't sent, because {name} is still finishing work it started. Your message and the agent you picked are kept. Press Stop to end it, or wait for it to finish, then send again.":
    "आपका संदेश नहीं भेजा गया, क्योंकि {name} अपना शुरू किया हुआ काम अभी पूरा कर रहा है। आपका संदेश और आपका चुना हुआ एजेंट सहेजे गए हैं। उसे खत्म करने के लिए रोकें दबाएँ, या उसके पूरा होने तक रुकें, फिर दोबारा भेजें।",
  "The other agent didn't get your message, because this conversation is too long to hand over to it. The conversation stays with its agent. To ask the other agent, start a new conversation with it and send your message there. To stay, send it here again.":
    "दूसरे एजेंट को आपका संदेश नहीं मिला, क्योंकि यह बातचीत उसे सौंपने के लिए बहुत लंबी है। बातचीत अपने एजेंट के साथ ही रहेगी। दूसरे एजेंट से पूछने के लिए, उसके साथ नई बातचीत शुरू करें और अपना संदेश वहाँ भेजें। यहीं रहने के लिए, संदेश यहाँ दोबारा भेजें।",
  // provisional (U3-err-agent.start_unreachable and its agents' lines): Hindi later
  "Claude didn't start, because Clauseo couldn't reach Claude from this computer, though it's online. A slow connection, or your firm's network blocking it, can cause this. Your question is kept. Try again in a moment. If it keeps happening, show this to whoever looks after this computer.":
    null,
  "ChatGPT didn't start, because Clauseo couldn't reach ChatGPT from this computer, though it's online. A slow connection, or your firm's network blocking it, can cause this. Your question is kept. Try again in a moment. If it keeps happening, show this to whoever looks after this computer.":
    null,
  "{name} didn't start, because Clauseo couldn't reach {name} from this computer, though it's online. A slow connection, or your firm's network blocking it, can cause this. Your question is kept. Try again in a moment. If it keeps happening, show this to whoever looks after this computer.":
    null,
  "The AI didn't start, because Clauseo couldn't reach it from this computer, though it's online. A slow connection, or your firm's network blocking it, can cause this. Your question is kept. Try again in a moment. If it keeps happening, show this to whoever looks after this computer.":
    null,
} as const;

/** The sentences' Hindi, for the copy review to tell hi: later from Hindi. */
export const errorHindi: Hindi<keyof typeof HI> = HI;
