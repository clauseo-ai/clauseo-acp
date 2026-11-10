// The Hindi of words-settings.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-settings.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // the window and its sections
  Sections: "अनुभाग",
  Language: "भाषा",
  Appearance: "दिखावट",
  About: "परिचय",
  "Plan and usage": "प्लान और उपयोग",
  Privacy: "निजता",
  // what each section says beside its name, at a glance
  "{theme}, text size {n}": "{theme}, अक्षरों का आकार {n}",
  "Version {version}": "संस्करण {version}",
  "Released {date}": "{date} को जारी",
  "{n}% of the 5-hour limit left": "5 घंटे की सीमा का {n}% बाकी",
  "Pay as you go": "उपयोग के अनुसार भुगतान",
  // a row the firm's policy locks (the old Hindi said "your organisation")
  "Managed by firm": "फ़र्म द्वारा प्रबंधित",
  // each section's one line
  "The language of every word on screen. Papers, quotes and case names stay as filed.":
    "स्क्रीन के हर शब्द की भाषा। दस्तावेज़, उद्धरण और मामलों के नाम वैसे ही रहते हैं जैसे दर्ज हैं।",
  "How Clauseo looks on this computer.": "इस कंप्यूटर पर Clauseo कैसा दिखे।",
  "What each plan has left, and when it resets.": "हर प्लान में कितना बाकी है, और वह कब रीसेट होता है।",
  // plan and usage: the plan's own words are the limits' (words-limits.ts)
  "Updated at {time}": "{time} बजे अपडेट किया गया",
  "No limits to show. Each question is billed to the key's account.":
    "दिखाने को कोई सीमा नहीं। हर प्रश्न का शुल्क उसी key के खाते पर लगता है।",
  "Checking what's left…": "कितना बाकी है, इसकी जाँच हो रही है…",
  "Set up Claude or ChatGPT to see what each plan has left.":
    "हर प्लान में कितना बाकी है, यह देखने के लिए Claude या ChatGPT सेट अप करें।",
  // language and appearance
  "Changes at once, and everything stays where it was.": "तुरंत बदलती है, और सब कुछ वहीं रहता है जहाँ था।",
  Day: "दिन",
  Night: "रात",
  // appearance's preview: the room in small, as the choices make it
  "How it looks": "यह कैसा दिखता है",
  "A strong case, because the complaint names Mr. Sharma only as a director.":
    "मामला मज़बूत है, क्योंकि शिकायत में श्री शर्मा को केवल निदेशक के रूप में नामित किया गया है।",
  // privacy: what leaves this computer, and the tools' switches
  "What leaves this computer": "इस कंप्यूटर से क्या बाहर जाता है",
  "To {name}, on your plan": "{name} को, आपके प्लान से",
  // the plans' group note: what goes to each plan, said once
  "Questions, the papers' parts they need, web searches.":
    "प्रश्न, उनके लिए ज़रूरी दस्तावेज़ों के अंश, वेब खोजें।",
  // behind an info icon: the button's name, then what it holds
  "More about Brave Search": "Brave Search के बारे में और",
  "Anthropic lists Brave Search as its web search subprocessor.":
    "Anthropic, Brave Search को अपने वेब खोज सब-प्रोसेसर के रूप में सूचीबद्ध करता है।",
  // under the rows: what Clauseo fetches by itself when it starts (D-064)
  "When it starts, Clauseo asks Claude or ChatGPT how much of your plan is left. No papers or questions go with it.":
    "शुरू होने पर Clauseo, Claude या ChatGPT से पूछता है कि आपके प्लान में कितना बाकी है। इसके साथ कोई दस्तावेज़ या प्रश्न नहीं जाते।",
  "To the websites Clauseo visits": "Clauseo जिन वेबसाइटों पर जाता है, उन्हें",
  "About websites": "वेबसाइटों के बारे में",
  "Websites see Clauseo's visits, as they see any browser's.":
    "वेबसाइटें Clauseo का आना वैसे ही देखती हैं, जैसे किसी भी ब्राउज़र का।",
  // the tools' switches; the group's note, the browser's reason and the lock's are the room's (words-tools.ts,
  // words-tools-line.ts)
  "On the web": "वेब पर",
  "Web search": "वेब खोज",
  "Search the web and read pages.": "वेब पर खोजें और पेज पढ़ें।",
  Browser: "ब्राउज़र",
  "Runs on this computer.": "इसी कंप्यूटर पर चलता है।",
  // Plans (UD2): each plan's block, its confirms, and the other ways in; English only for now
  Plans: "प्लान",
  "The plans Clauseo works with. Each signs in through its own page in your browser.":
    "वे प्लान जिनके साथ Clauseo काम करता है। हर प्लान आपके ब्राउज़र में अपने पेज से साइन इन करता है।",
  "Sign out": "साइन आउट करें",
  "Sign out of {name}?": "{name} से साइन आउट करें?",
  "Your matters and conversations stay on this computer. Questions wait until you sign in again.":
    "आपके मामले और बातचीत इसी कंप्यूटर पर रहते हैं। दोबारा साइन इन करने तक प्रश्न प्रतीक्षा करेंगे।",
  "Signed out of {name}.": "{name} से साइन आउट हो गया।",
  "Stop using it here": "यहाँ इस्तेमाल बंद करें",
  "Stop using {app}'s sign-in here?": "यहाँ {app} का साइन इन इस्तेमाल करना बंद करें?",
  "This doesn't sign you out of {app}. Clauseo stops using its sign-in, and you can sign in here instead.":
    "इससे आप {app} से साइन आउट नहीं होते। Clauseo उसका साइन इन इस्तेमाल करना बंद करता है, और आप यहाँ अलग से साइन इन कर सकते हैं।",
  "Look for my {app} sign-in": "मेरा {app} साइन इन खोजें",
  "Uses your ChatGPT plan.": "आपके ChatGPT प्लान का उपयोग करता है।",
  "Add ChatGPT": "ChatGPT जोड़ें",
  // the same card for Claude, when Claude isn't added yet (the plans side by side)
  "Uses your Claude plan.": "आपके Claude प्लान का उपयोग करता है।",
  "Add Claude": "Claude जोड़ें",
  "Needs a download of about {mb} MB. Signing in happens in your browser.":
    "लगभग {mb} MB का डाउनलोड ज़रूरी है। साइन इन आपके ब्राउज़र में होता है।",
  "Signing in happens in your browser.": "साइन इन आपके ब्राउज़र में होता है।",
  "Other ways in": "जुड़ने के दूसरे तरीके",
  // the key's help under each plan's card names that plan only
  "A key from {name}, yours or your firm's. Billed to that key's account.":
    "{name} की key, आपकी या आपकी फ़र्म की। शुल्क उसी key के खाते पर लगता है।",
  "Change key": "key बदलें",
  "Remove key": "key हटाएँ",
  "Remove the API key?": "API key हटाएँ?",
  "Questions wait until you sign in or add a key again.":
    "दोबारा साइन इन करने या key जोड़ने तक प्रश्न प्रतीक्षा करेंगे।",
  "Sign in again": "दोबारा साइन इन करें",
  "Your firm's address": "आपकी फ़र्म का पता",
  "Your IT team gives you this. Questions go through your firm's own account, on its terms.":
    "यह आपकी IT टीम देती है। प्रश्न आपकी फ़र्म के अपने खाते से, उसकी शर्तों पर जाते हैं।",
  Connect: "कनेक्ट करें",
  "Remove address": "पता हटाएँ",
  // what a search for the CLI's login found, once the host says (stage D's FoundLogin none_found and reuse_off)
  "No {app} sign-in on this computer.": "इस कंप्यूटर पर {app} का कोई साइन इन नहीं।",
  "Clauseo isn't using {app}'s sign-in here.": "Clauseo यहाँ {app} का साइन इन इस्तेमाल नहीं कर रहा।",
  // UC: the Safe workspace (settings.html#workspace); the design's own words keep its Hindi
  "Safe workspace": "सुरक्षित कार्यक्षेत्र",
  "On for every matter": "हर मामले के लिए चालू",
  "Not protected": "संरक्षित नहीं",
  "Set up again": "फिर से सेट करें",
  "Set up on {date}, with Windows' permission.": "{date} को Windows की अनुमति से सेट किया गया।",
  "Clauseo can't show this computer's Safe workspace here yet.":
    "Clauseo अभी यहाँ इस कंप्यूटर का सुरक्षित कार्यक्षेत्र नहीं दिखा सकता।",
  "macOS keeps Clauseo inside each matter's folder.":
    "macOS, Clauseo को हर मामले के फ़ोल्डर के भीतर रखता है।",
  "Windows keeps Clauseo inside each matter's folder.":
    "Windows, Clauseo को हर मामले के फ़ोल्डर के भीतर रखता है।",
  "Set up with Windows' permission.": "Windows की अनुमति से सेट अप किया गया।",
  "The Safe workspace isn't set up on this computer yet, so Clauseo asks before changing anything outside a matter's folder.":
    "इस कंप्यूटर पर सुरक्षित कार्यक्षेत्र अभी तैयार नहीं है, इसलिए Clauseo किसी मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "Windows didn't allow the set-up, so Clauseo asks before changing anything outside a matter's folder.":
    "Windows ने सेटअप की अनुमति नहीं दी, इसलिए Clauseo किसी मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "The Safe workspace couldn't be set up on this computer, so Clauseo asks before changing anything outside a matter's folder.":
    "इस कंप्यूटर पर सुरक्षित कार्यक्षेत्र तैयार नहीं हो सका, इसलिए Clauseo किसी मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "This computer can't run the Safe workspace, so Clauseo asks before changing anything outside a matter's folder.":
    "यह कंप्यूटर सुरक्षित कार्यक्षेत्र नहीं चला सकता, इसलिए Clauseo किसी मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "The Safe workspace isn't available on Linux yet, so Clauseo asks before changing anything outside a matter's folder.":
    "सुरक्षित कार्यक्षेत्र अभी Linux पर उपलब्ध नहीं है, इसलिए Clauseo किसी मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "The Safe workspace isn't available on Windows on ARM yet, so Clauseo asks before changing anything outside a matter's folder.":
    "सुरक्षित कार्यक्षेत्र अभी ARM वाले Windows पर उपलब्ध नहीं है, इसलिए Clauseo किसी मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "This version of Clauseo doesn't have the Safe workspace yet. It comes in an update. Until then, Clauseo asks before changing anything outside a matter's folder.":
    "Clauseo के इस संस्करण में अभी सुरक्षित कार्यक्षेत्र नहीं है। यह एक अपडेट में आएगा। तब तक Clauseo किसी मामले के फ़ोल्डर के बाहर कुछ भी बदलने से पहले पूछता है।",
  "Your firm requires the Safe workspace, so Clauseo doesn't answer on this computer until it's set up.":
    "आपकी फ़र्म सुरक्षित कार्यक्षेत्र ज़रूरी मानती है, इसलिए जब तक यह तैयार नहीं होता, Clauseo इस कंप्यूटर पर उत्तर नहीं देता।",
  "Your firm doesn't allow Clauseo to work without asking on this computer.":
    "आपकी फ़र्म इस कंप्यूटर पर Clauseo को बिना पूछे काम करने की अनुमति नहीं देती।",
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const settingsWords: Hindi<keyof typeof HI> = HI;
