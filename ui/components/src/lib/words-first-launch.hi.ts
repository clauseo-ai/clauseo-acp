// The Hindi of words-first-launch.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-first-launch.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // the first screen (design)
  "Clauseo works on your matters with the Claude or ChatGPT plan you already pay for.":
    "Clauseo आपके मामलों पर उसी Claude या ChatGPT प्लान से काम करता है, जिसका भुगतान आप पहले से करते हैं।",
  "Open the first matter": "पहला मामला खोलें",
  "Open the folder for a matter": "किसी मामले का फ़ोल्डर खोलें",
  "Clauseo reads its papers on this computer first. Apart from anonymous counts, nothing leaves it until you ask.":
    "Clauseo पहले इसी कंप्यूटर पर उसके दस्तावेज़ पढ़ता है। गुमनाम गिनती के सिवा, आपके पूछने तक कुछ भी बाहर नहीं जाता।",
  "Start a new matter": "नया मामला शुरू करें",
  "Send anonymous counts": "गुमनाम गिनती भेजें",
  "Send anonymous counts.": "गुमनाम गिनती भेजें।",
  "How many questions, which features, how long answers take. Never your papers, your questions or the answers.":
    "कितने प्रश्न, कौन-सी सुविधाएँ, और उत्तर कितना समय लेते हैं। आपके दस्तावेज़, आपके प्रश्न या उत्तर कभी नहीं।",
  "What's sent": "क्या भेजा जाता है",

  // getting the agent ready, on the working line (design)
  "Getting {name} ready. About {mb} MB.": "{name} तैयार हो रहा है। लगभग {mb} MB।",
  "{name} is ready": "{name} तैयार है",
  "Use the system proxy": "सिस्टम की proxy इस्तेमाल करें",
  "Ask your IT team": "अपनी IT टीम से कहें",
  "Message copied": "संदेश कॉपी हो गया",
  "Clauseo needs to download {name} to work. Please allow these addresses on the firm's network: {list}.":
    "काम करने के लिए Clauseo को {name} डाउनलोड करना है। कृपया फ़र्म के नेटवर्क पर ये पते अनुमत करें: {list}।",
  "{name} needs {need} MB free on this computer, and {free} MB is free.":
    "{name} को इस कंप्यूटर पर {need} MB खाली जगह चाहिए, और अभी {free} MB खाली है।",
  "{name} couldn't be downloaded.": "{name} डाउनलोड नहीं हो सका।",
  "{name} couldn't be downloaded. The log is copied, with nothing from your matters in it.":
    "{name} डाउनलोड नहीं हो सका। लॉग कॉपी हो गया है, उसमें आपके मामलों का कुछ नहीं है।",
  "Try again": "फिर से प्रयास करें",
  Details: "विवरण",
  // the download done, while npm puts it together: no bytes to count and no time to promise (new)
  "Setting up {name} on this computer.": null,

  // the fine print, up front (design)
  "Your papers stay on this computer. {name} sees what each question sends it.":
    "आपके दस्तावेज़ इसी कंप्यूटर पर रहते हैं। हर प्रश्न के साथ जो भेजा जाता है, वही {name} देखता है।",

  // the plan, in the composer (design)
  "Sign in to {name} to ask about this matter": "इस मामले के बारे में पूछने के लिए {name} में साइन इन करें",
  "Sign in": "साइन इन करें",
  "Use {name} instead": "इसके बजाय {name} इस्तेमाल करें",
  "Use an API key or my firm's account": "API key या अपनी फ़र्म का खाता इस्तेमाल करें",
  "Your browser opens when you reconnect.": "दोबारा कनेक्ट होते ही आपका ब्राउज़र खुलेगा।",
  "API key": "API key",
  "Use this key": "यह key इस्तेमाल करें",
  "A key from Claude or ChatGPT, or one from your firm's IT team. Billed to the key's own account.":
    "Claude या ChatGPT की key, या आपकी फ़र्म की IT टीम से मिली key। शुल्क उसी key के खाते पर लगता है।",
  "Using an API key, billed to its own account": "API key से, उसी के खाते पर शुल्क",
  // Sign in pressed before the install is ready, in place of the design's "Sign in at {time}" (new)
  "Your browser opens when {name} is ready.": null,
  // where the key is kept, by display_info's platform, and a computer with no keyring (new)
  "Kept in this Mac's Keychain.": null,
  "Kept in Windows Credential Manager.": null,
  "Kept in this computer's keyring.": null,
  "This computer has no keyring to keep an API key in.": null,
  "Sign in with an Anthropic Console account": null,
  "Using your firm's account": null,

  // signing in, in the browser (design)
  "Finish signing in in your browser": "अपने ब्राउज़र में साइन इन पूरा करें",
  "Didn't open?": "ब्राउज़र नहीं खुला?",
  "Got a code instead?": "इसके बजाय कोड मिला?",
  "Paste it": "उसे पेस्ट करें",
  "Code from your browser": "ब्राउज़र से मिला कोड",
  "Clauseo passes the code to {name} and keeps no copy.":
    "Clauseo यह कोड {name} को दे देता है और उसकी कोई प्रति नहीं रखता।",
  Send: "भेजें",
  "Signed in as {email} · {plan}": "{email} के रूप में साइन इन · {plan}",
  "The sign-in link expired at {time}.": "साइन इन का लिंक {time} बजे समाप्त हो गया।",
  "See {name}'s plans": "{name} के प्लान देखें",
  "Use another account": "दूसरा खाता इस्तेमाल करें",
  "Clauseo works with a Claude Pro or Max plan.":
    "Clauseo, Claude Pro या Max प्लान के साथ काम करता है।",
  "Clauseo works with a paid ChatGPT plan.": "Clauseo, सशुल्क ChatGPT प्लान के साथ काम करता है।",
  // signed in with less than the event can say (new)
  "Signed in as {email}": null,
  "Signed in to {name}": null,

  // a login already on this computer, said before it is used (new; Rohan, 2026-11-02)
  "Clauseo found your {name} account ({email}) from {app} and will use it here.": null,
  "Clauseo found your {name} account from {app} and will use it here.": null,
  "Use it": null,
  "Sign in with a different account": null,
  "A different account signs in to Clauseo only. {app} stays as it is.": null,
  "Signed in through {app} as {email} · {plan}": null,
  "Signed in through {app} as {email}": null,
  "Signed in through {app}": null,

  // a login seen that can't be shared, or reuse that stopped (new)
  "Claude Code on this computer signs in through your own Claude settings, which Clauseo keeps out of your matters.":
    null,
  "Codex on this computer keeps its sign-in where Clauseo can't share it.": null,
  "Clauseo's safe workspace can't share Claude Code's sign-in on Windows yet.": null,
  "{app} on this computer is signed out, so Clauseo stopped using its sign-in.": null,
  "{app} on this computer now uses another account, so Clauseo stopped using its sign-in.": null,
  "Codex's sign-in on this computer changed, so Clauseo stopped using it.": null,
  "{app}'s sign-in stopped working here, so Clauseo stopped using it.": null,

  // ChatGPT's device code, offered only after the browser didn't work (new)
  "Use a code instead": null,
  "Sign in to ChatGPT on that page and enter this code.": null,
  "The code works until {time}.": null,
  Copy: null,
  "Open page": null,
  "To turn it on, open ChatGPT, then Settings, then Security, and turn on Enable device code sign-in for Codex, Excel, PowerPoint, and Word. Then choose Try again.":
    null,
  "Sign in in the browser instead": null,

  // the other ways a sign-in stops (new)
  "Sign-in didn't finish.": null,
  "Clauseo can't read your firm's settings for {name}, so it won't sign in until they can be read.":
    null,
  "Ask your IT team.": null,
} satisfies Hindi<string>;

/** Every string, English and Hindi (null: hi: later), for the tests that hold the voice and the copy review. */
export const firstLaunchWords: Hindi<keyof typeof HI> = HI;
