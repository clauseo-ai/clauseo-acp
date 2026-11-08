// The Hindi of words-connectors.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-connectors.ts imports this file's type alone.

export const HI = {
  // U15: the section's name and its line
  Connectors: "कनेक्टर",
  "Paid sources Clauseo can search while it answers.":
    "सशुल्क स्रोत, जिन्हें Clauseo उत्तर देते समय खोज सकता है।",
  // the connector's block: its name and what it covers
  "Indian Kanoon": "Indian Kanoon",
  "Judgments of the Supreme Court, the High Courts and tribunals, with statutes.":
    "सर्वोच्च न्यायालय, उच्च न्यायालयों और अधिकरणों के निर्णय, अधिनियमों सहित।",
  // not added; {price} is rupees() of the cheapest pack's gross price, GST included
  "Not added": "नहीं जोड़ा गया",
  "Credits are bought in packs, from {price}, including GST.":
    "क्रेडिट पैक में खरीदे जाते हैं, GST सहित {price} से शुरू।",
  Add: "जोड़ें",
  "Your browser opens to sign in.": "साइन इन के लिए आपका ब्राउज़र खुलेगा।",
  "Use an API key instead": "इसके बजाय API key इस्तेमाल करें",
  // signing in
  "Signing in": "साइन इन हो रहा है",
  "Finish signing in on the page your browser opened.": "ब्राउज़र में खुले पेज पर साइन इन पूरा करें।",
  "Open the sign-in page again": "साइन इन पेज फिर से खोलें",
  // signed in: the balance in credits ({credits} is count()); its worth in rupees waits for stage E's gross rate
  "Ready to search.": "खोज के लिए तैयार।",
  // signed in with a pasted key: the firm's key, not the lawyer's account
  "Using your firm's key.": "आपकी फ़र्म की key इस्तेमाल हो रही है।",
  "{credits} credits left": "{credits} क्रेडिट बाकी",
  // the balance's worth in rupees and when it was read (U15-15, U15-76); GST-inclusive, as every amount
  "Worth {rupees}.": "मूल्य {rupees}।",
  "As of {time}.": "{time} बजे के अनुसार।",
  "Balance not read yet.": "बैलेंस अभी नहीं पढ़ा गया।",
  "Check balance": "बैलेंस देखें",
  "Running low. Add credits to keep searching.": "क्रेडिट कम हैं। खोज जारी रखने के लिए क्रेडिट जोड़ें।",
  "Add credits": "क्रेडिट जोड़ें",
  "Sign out": "साइन आउट करें",
  Remove: "हटाएँ",
  // recent spend, credits only
  "Recent spend": "हाल का खर्च",
  Today: "आज",
  "{credits} credits": "{credits} क्रेडिट",
  "Nothing spent yet.": "अभी कुछ खर्च नहीं हुआ।",
  // out of credit, signed out, a revoked key, a failed sign-in
  "Out of credit": "क्रेडिट समाप्त",
  "No credits left. Case-law searches stop until you add more.":
    "कोई क्रेडिट बाकी नहीं। और जोड़ने तक निर्णयों की खोज रुकी रहेगी।",
  "Signed out": "साइन आउट हो गया",
  "Signed out. Sign in to search case law again.":
    "साइन आउट हो गया। निर्णय फिर से खोजने के लिए साइन इन करें।",
  "Sign in": "साइन इन करें",
  "Key no longer works": "key अब काम नहीं करती",
  "This API key no longer works. Add a new key, or sign in instead.":
    "यह API key अब काम नहीं करती। नई key जोड़ें, या इसके बजाय साइन इन करें।",
  "Add a new key": "नई key जोड़ें",
  "Sign-in failed": "साइन इन नहीं हो सका",
  "The sign-in didn't go through. Try again, or use an API key instead.":
    "साइन इन पूरा नहीं हुआ। फिर से प्रयास करें, या इसके बजाय API key इस्तेमाल करें।",
  // signed in now, but the sign-in could not be saved for the next start
  "You're signed in, but Clauseo couldn't save the sign-in on this computer. Sign in again after Clauseo restarts.":
    "आप साइन इन हैं, पर Clauseo इस कंप्यूटर पर साइन इन सहेज नहीं सका। Clauseo के फिर से शुरू होने के बाद दोबारा साइन इन करें।",
  "Try again": "फिर से प्रयास करें",
  Details: "विवरण",
  // a firm's policy turns connectors off: the side's note, then the block's line
  "Turned off by your firm": "आपकी फ़र्म ने बंद किया है",
  "Turned off by your firm.": "आपकी फ़र्म ने बंद किया है।",
  // the packs
  "Choose a pack": "पैक चुनें",
  "{credits} credits, {rupees}": "{credits} क्रेडिट, {rupees}",
  "Prices include GST.": "कीमतों में GST शामिल है।",
  "Continue to payment": "भुगतान पर जाएँ",
  Cancel: "रद्द करें",
  // the payment page
  "The payment page is open in your browser.": "भुगतान पेज आपके ब्राउज़र में खुला है।",
  "Your balance updates here once the payment goes through.":
    "भुगतान पूरा होते ही आपका बैलेंस यहाँ अपडेट होगा।",
  "Open the payment page again": "भुगतान पेज फिर से खोलें",
  // how the last recharge ended, a quiet line in the section while it is set
  "Payment received.": "भुगतान मिल गया।",
  "That payment page expired. Add credits to try again.":
    "वह भुगतान पेज समाप्त हो गया। फिर से प्रयास करने के लिए क्रेडिट जोड़ें।",
  "The payment was cancelled.": "भुगतान रद्द हो गया।",
  // the API key
  "API key": "API key",
  "Your firm's key for Clauseo's case-law service. Kept on this computer.":
    "Clauseo की निर्णय-खोज सेवा के लिए आपकी फ़र्म की key। इसी कंप्यूटर पर रखी है।",
  "Add key": "key जोड़ें",
  // what each search sends; {name} is the agents' names (Claude, ChatGPT)
  "What each search sends": "हर खोज क्या भेजती है",
  "The words of each search go to Clauseo's server, which sends them on to Indian Kanoon.":
    "हर खोज के शब्द Clauseo के सर्वर पर जाते हैं, जो उन्हें Indian Kanoon को भेजता है।",
  "Papers aren't sent. Clauseo asks {name} to keep client names and confidential facts out of its searches.":
    "दस्तावेज़ नहीं भेजे जाते। Clauseo {name} से कहता है कि वह अपनी खोजों में मुवक्किलों के नाम और गोपनीय तथ्य न डाले।",
  "Clauseo's server deletes the words of each search after 7 days.":
    "Clauseo का सर्वर हर खोज के शब्द 7 दिन बाद हटा देता है।",
  "Indian Kanoon sees Clauseo's server, never you.":
    "Indian Kanoon को Clauseo का सर्वर दिखता है, आप कभी नहीं।",
  "Each search and each judgment read costs credits.": "हर खोज और पढ़े गए हर निर्णय पर क्रेडिट लगते हैं।",
  // the lawyer's own off, and the way to turn it off
  "Turned off on this computer": "इस कंप्यूटर पर बंद है",
  "Turned off on this computer.": "इस कंप्यूटर पर बंद है।",
  "Turn on": "चालू करें",
  "On this computer": "इस कंप्यूटर पर",
  "Case-law searches": "निर्णयों की खोज",
  "Turning off stops case-law searches on this computer. Your credits stay.":
    "बंद करने पर इस कंप्यूटर पर निर्णयों की खोज रुक जाती है। आपके क्रेडिट बने रहते हैं।",
  "Turn off": "बंद करें",
  // the pay link that came after the press's 30 seconds: nothing opens until this is pressed
  "Open the payment page": "भुगतान पेज खोलें",
} as const;
