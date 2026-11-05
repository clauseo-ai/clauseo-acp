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
  "Clauseo can't start Claude safely here. Reinstall Clauseo.": null,
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
  "Clauseo can't start ChatGPT safely here. Reinstall Clauseo.": null,
  "Clauseo can't start {name} safely here. Reinstall Clauseo.": null,
  "Clauseo can't start the AI safely here. Reinstall Clauseo.": null,
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
  "You're offline. Setting up waits for the connection.": null,
  "Your network is blocking the download.": null,
  "There isn't enough free space on this computer for the download.": null,
  "Setting up didn't finish. Try again.": null,
  "Sign-in was cancelled in the browser.": null,
  "The sign-in link expired. Try again.": null,
  "This account has no plan that works with Clauseo.": null,
  "Sign-in with a code is turned off for this ChatGPT account.": null,
  "Another program on this computer is using ChatGPT's sign-in.": null,
  "Sign-in didn't finish. Try again.": null,
  "Clauseo can't read your firm's sign-in settings, so it won't sign in until they can be read.":
    null,
  // hi: later
  "Codex can't work on a network drive. Move the matter to this computer.": null,
  "Codex can't work on a drive without file permissions. Move the matter to this computer.": null,
} as const;

/** The sentences' Hindi, for the copy review to tell hi: later from Hindi. */
export const errorHindi: Hindi<keyof typeof HI> = HI;
