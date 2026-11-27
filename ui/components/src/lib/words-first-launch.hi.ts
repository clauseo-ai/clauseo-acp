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
  // the welcome's counts line, crash reports with them (0060, UC-750); the line above is Settings'
  "How many questions, which features, how long answers take, and a short report if Clauseo crashes. Never your papers, your questions or the answers.":
    "कितने प्रश्न, कौन-सी सुविधाएँ, उत्तर कितना समय लेते हैं, और Clauseo क्रैश हो तो एक छोटी रिपोर्ट। आपके दस्तावेज़, आपके प्रश्न या उत्तर कभी नहीं।",
  "What's sent": "क्या भेजा जाता है",

  // getting the agent ready, on the working line (design): its size first, then the download's
  // bytes, then the set-up
  "Needs a download of about {mb} MB": "लगभग {mb} MB का डाउनलोड ज़रूरी है",
  "Getting {name} ready… {done} MB of {total} MB": "{name} तैयार हो रहा है… {total} MB में से {done} MB",
  // the live region's words for the download, which hears no bytes (new)
  "Getting {name} ready…": "{name} तैयार हो रहा है…",
  "{name} is ready": "{name} तैयार है",
  "Use the system proxy": "सिस्टम की प्रॉक्सी इस्तेमाल करें",
  "Copy details for IT": "IT के लिए विवरण कॉपी करें",
  // UC-144: a blocked download's copy button, which copies the addresses (UD1-17)
  "Copy addresses": "पते कॉपी करें",
  "Message copied": "संदेश कॉपी हो गया",
  // the facts in a note for IT (words-first-launch.ts's itNote), each "key: value" (new,
  // provisional)
  "Code: {code}": "कोड: {code}",
  "Reason: {reason}": "कारण: {reason}",
  "Site: {site}": "वेबसाइट: {site}",
  "Addresses: {list}": "पते: {list}",
  "Program: {path}": "प्रोग्राम: {path}",
  "Program hash: {hash}": "प्रोग्राम का हैश: {hash}",
  "Clauseo version: {version}": "Clauseo संस्करण: {version}",
  "Clauseo for firm IT: {url}": "फ़र्म IT के लिए Clauseo: {url}",
  "Clauseo needs to download {name}. These addresses must be reachable from this computer: {list}.":
    "Clauseo को {name} डाउनलोड करना है। ये पते इस कंप्यूटर से पहुँच में होने चाहिए: {list}।",
  "{name} needs {need} MB free on this computer, and {free} MB is free.":
    "{name} को इस कंप्यूटर पर {need} MB खाली जगह चाहिए, और अभी {free} MB खाली है।",
  "{name} couldn't be downloaded.": "{name} डाउनलोड नहीं हो सका।",
  "{name} couldn't be downloaded. The log is copied, with nothing from your matters in it.":
    "{name} डाउनलोड नहीं हो सका। लॉग कॉपी हो गया है, उसमें आपके मामलों का कुछ नहीं है।",
  "Try again": "फिर से प्रयास करें",
  Details: "विवरण",
  // the download done, while npm puts it together: no bytes to count and no time to promise (new)
  "Setting up {name}…": "{name} सेट अप हो रहा है…",
  // agent.install_offline while the window says it is online: the download's address can't be reached, so not
  // "You're offline" (new; the plans' line and the document tools' line)
  "Clauseo can't reach the download right now. It tries again by itself.":
    "Clauseo अभी डाउनलोड तक नहीं पहुँच पा रहा। यह अपने-आप फिर से कोशिश करेगा।",

  // the fine print, up front (design)
  "Your papers stay on this computer. {name} sees the parts it reads to answer you.":
    "आपके दस्तावेज़ इसी कंप्यूटर पर रहते हैं। {name} उतना ही देखता है, जितना वह आपको जवाब देने के लिए पढ़ता है।",

  // the plan, in the composer (design)
  "Sign in with Claude or ChatGPT to ask about this matter":
    "इस मामले के बारे में पूछने के लिए Claude या ChatGPT से साइन इन करें",
  "Sign in": "साइन इन करें",
  "Use {name} instead": "इसके बजाय {name} इस्तेमाल करें",
  // UC-141: the quiet way to the key panel, in Connectors' Hindi (U15-09)
  "Use an API key instead": "इसके बजाय API key इस्तेमाल करें",
  "Your browser opens when you reconnect.": "दोबारा कनेक्ट होते ही आपका ब्राउज़र खुलेगा।",
  // the install waits while the window says it is online: the download can't be reached, so not "reconnect" (new)
  "Your browser opens when setting up can continue.": "सेटअप आगे बढ़ने पर आपका ब्राउज़र खुलेगा।",
  // UC-139 and UC-140: the field names the key by its maker; "API key" stays in Latin script (D-069)
  "Anthropic API key": "Anthropic API key",
  "OpenAI API key": "OpenAI API key",
  "Use this key": "यह key इस्तेमाल करें",
  // UC-145 and UC-146: the key panel's help names the key's maker; "Anthropic Console", "OpenAI" and "API key" stay
  // in Latin script (D-069)
  "A key from the Anthropic Console. Billed to that account.":
    "Anthropic Console की key। शुल्क उसी खाते पर लगता है।",
  "A key from the OpenAI platform. Billed to that account.":
    "OpenAI platform की key। शुल्क उसी खाते पर लगता है।",
  "Using an API key, billed to its own account": "API key से, उसी के खाते पर शुल्क",
  // Sign in pressed before the install is ready, in place of the design's "Sign in at {time}" (new)
  "Your browser opens when {name} is ready.": "{name} तैयार होते ही आपका ब्राउज़र खुलेगा।",
  // the plan ready while the other plan's sign-in is under way: sign-ins go one at a time (new, UC-153)
  "Your browser opens for {name} after {other}'s sign-in.":
    "{other} का साइन इन पूरा होते ही {name} के लिए आपका ब्राउज़र खुलेगा।",
  // the waiting plan's press that ends the other plan's sign-in ahead of it (new, UC-294)
  "Cancel {name}'s sign-in": "{name} का साइन इन रद्द करें",
  // where the key is kept, by display_info's platform, and a computer with no keyring (new)
  "Kept in this Mac's Keychain.": "इस Mac के Keychain में रखी है।",
  "Kept in Windows Credential Manager.": "Windows Credential Manager में रखी है।",
  "Kept in this computer's keyring.": "इस कंप्यूटर के keyring में रखी है।",
  "This computer has no keyring to keep an API key in.":
    "इस कंप्यूटर में API key रखने के लिए कोई keyring नहीं है।",
  "Sign in with an Anthropic Console account": "Anthropic Console खाते से साइन इन करें",
  "Using your firm's account": "आपकी फ़र्म का खाता इस्तेमाल हो रहा है",

  // signing in, in the browser (design)
  "Finish signing in with your browser.": "अपने ब्राउज़र में साइन इन पूरा करें",
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
  "Signed in as {email}": "{email} के रूप में साइन इन",
  "Signed in to {name}": "{name} में साइन इन",

  // a login already on this computer, said before it is used (new; Rohan, 2026-11-02)
  "Clauseo found your {name} account ({email}) from {app} and will use it here.":
    "Clauseo को {app} से आपका {name} खाता ({email}) मिला है। वह इसे यहाँ इस्तेमाल करेगा।",
  "Clauseo found your {name} account from {app} and will use it here.":
    "Clauseo को {app} से आपका {name} खाता मिला है। वह इसे यहाँ इस्तेमाल करेगा।",
  "Use it": "इसे इस्तेमाल करें",
  "Sign in with a different account": "किसी दूसरे खाते से साइन इन करें",
  "A different account signs in to Clauseo only. {app} stays as it is.":
    "दूसरा खाता केवल Clauseo में साइन इन करता है। {app} जैसा है वैसा रहता है।",
  "Signed in through {app} as {email} · {plan}": "{app} के ज़रिए {email} के रूप में साइन इन · {plan}",
  "Signed in through {app} as {email}": "{app} के ज़रिए {email} के रूप में साइन इन",
  "Signed in through {app}": "{app} के ज़रिए साइन इन",

  // a login seen that can't be shared, or reuse that stopped (new)
  "Claude Code on this computer signs in through your own Claude settings, which Clauseo keeps out of your matters.":
    "इस कंप्यूटर पर Claude Code आपकी अपनी Claude सेटिंग्स से साइन इन करता है, जिन्हें Clauseo आपके मामलों से दूर रखता है।",
  "Codex on this computer keeps its sign-in where Clauseo can't share it.":
    "इस कंप्यूटर पर Codex अपना साइन इन ऐसी जगह रखता है जहाँ से Clauseo उसे साझा नहीं कर सकता।",
  "Clauseo's Safe workspace can't share Claude Code's sign-in on Windows yet.":
    "Clauseo का सुरक्षित कार्यक्षेत्र अभी Windows पर Claude Code का साइन-इन साझा नहीं कर सकता।",
  "{app} on this computer is signed out, so Clauseo stopped using its sign-in.":
    "इस कंप्यूटर पर {app} से साइन आउट हो गया है, इसलिए Clauseo ने उसका साइन इन इस्तेमाल करना बंद कर दिया।",
  "{app} on this computer now uses another account, so Clauseo stopped using its sign-in.":
    "इस कंप्यूटर पर {app} अब दूसरा खाता इस्तेमाल करता है, इसलिए Clauseo ने उसका साइन इन इस्तेमाल करना बंद कर दिया।",
  "Codex's sign-in on this computer changed, so Clauseo stopped using it.":
    "इस कंप्यूटर पर Codex का साइन इन बदल गया, इसलिए Clauseo ने उसे इस्तेमाल करना बंद कर दिया।",
  "{app}'s sign-in stopped working here, so Clauseo stopped using it.":
    "{app} का साइन इन यहाँ काम नहीं कर रहा, इसलिए Clauseo ने उसे इस्तेमाल करना बंद कर दिया।",
  // "Use it" (or a later start) found the login, but it didn't work here (FoundLogin reuse_unusable)
  "Clauseo couldn't use your {app} sign-in. It may have expired, so sign in here instead.":
    "Clauseo आपके {app} साइन इन का इस्तेमाल नहीं कर सका। हो सकता है वह समाप्त हो गया हो, इसलिए इसके बजाय यहाँ साइन इन करें।",

  // ChatGPT's device code, offered only after the browser didn't work (new)
  "Use a code instead": "इसके बजाय कोड इस्तेमाल करें",
  "Sign in with ChatGPT on that page and enter this code.":
    "उस पेज पर ChatGPT से साइन इन करें और यह कोड डालें।",
  "The code works until {time}.": "यह कोड {time} बजे तक काम करेगा।",
  Copy: "कॉपी करें",
  "Open page": "पेज खोलें",
  "To turn it on, open ChatGPT, then Settings, then Security, and turn on Enable device code sign-in for Codex, Excel, PowerPoint, and Word. Then choose Try again.":
    "इसे चालू करने के लिए ChatGPT खोलें, फिर Settings, फिर Security, और Enable device code sign-in for Codex, Excel, PowerPoint, and Word चालू करें। इसके बाद 'फिर से प्रयास करें' चुनें।",
  "Sign in in the browser instead": "इसके बजाय ब्राउज़र में साइन इन करें",

  // the other ways a sign-in stops (new)
  "Sign-in didn't finish.": "साइन इन पूरा नहीं हुआ।",
  "Clauseo can't read your firm's settings for {name}, so it won't sign in until they can be read.":
    "Clauseo {name} के लिए आपकी फ़र्म की सेटिंग्स नहीं पढ़ पा रहा, इसलिए उनके पढ़े जाने तक साइन इन नहीं करेगा।",
  "Your firm manages these settings.": "ये सेटिंग्स आपकी फ़र्म प्रबंधित करती है।",
  // the running page's signin-expired card, a firm's account that refused the question
  "Your firm manages this account.": "यह खाता आपकी फ़र्म प्रबंधित करती है।",

  // the sign-in status check still running, at start and through its retries (new; provisional)
  "Checking whether you're signed in to {name}…":
    "जाँच हो रही है कि आपने {name} में साइन इन किया है या नहीं…",

  // the chosen plan's install went missing after Clauseo installed it (#859), and its one press, which installs it
  // the normal way; {plan} is the plan's name, Claude or ChatGPT (new; provisional)
  "{plan} isn't installed on this computer any more.": "{plan} अब इस कंप्यूटर पर इंस्टॉल नहीं है।",
  "Install {plan}": "{plan} इंस्टॉल करें",

  // switching accounts: Clauseo still stopping a login it may no longer use (new, 0048)
  "Switching accounts.": "खाता बदला जा रहा है।",
  "Clauseo is still stopping the old login. It keeps trying by itself, so there's nothing you need to do.":
    "Clauseo अभी पुराना लॉगिन बंद कर रहा है। यह अपने आप प्रयास करता रहता है, इसलिए आपको कुछ नहीं करना है।",
  "If this goes on, quit Clauseo and open it again.":
    "अगर ऐसा चलता रहे, तो Clauseo बंद करके फिर से खोलें।",
  "Clauseo is still switching accounts. Try again in a moment.":
    "Clauseo अभी खाता बदल रहा है। थोड़ी देर में फिर से प्रयास करें।",

  // UC: the Safe workspace step, the design's own words with its Hindi (UC-10, UC-13, UC-16); under
  // design B the Windows screen stands only for a set-up after an in-app Remove
  "Windows asks once for permission to set up the Safe workspace.":
    "सुरक्षित कार्यक्षेत्र तैयार करने के लिए Windows एक बार अनुमति माँगता है।",
  "Not now": "अभी नहीं",
  Settings: "सेटिंग्स",

  // both plans side by side on the first screen and in the matter's sign-in box (new; provisional)
  "Sign in with the plan you pay for: Claude, ChatGPT or both.":
    "जिस प्लान का भुगतान आप करते हैं, उससे साइन इन करें: Claude, ChatGPT या दोनों।",
  "Your papers stay on this computer. Claude or ChatGPT sees the parts it reads to answer you.":
    "आपके दस्तावेज़ इसी कंप्यूटर पर रहते हैं। Claude या ChatGPT उतना ही देखता है, जितना वह आपको जवाब देने के लिए पढ़ता है।",
  "Sign in with {name}": "{name} से साइन इन करें",
  // the plan chosen for a conversation, not set up on this computer (PK): its one act, in the composer
  "{name} isn't set up on this computer.": "{name} इस कंप्यूटर पर सेट अप नहीं है।",
  "Set up {name}": "{name} सेट अप करें",
  // a plan's row on the first screen, after its name and " · "
  "needs a download of about {mb} MB": "लगभग {mb} MB का डाउनलोड ज़रूरी है",
  // UD1-97: under Claude's "Finish signing in with your browser.": claude.ai's own sign-in page opens every time
  "Claude may ask you to sign in again in your browser. Use your Claude account.":
    "Claude आपके ब्राउज़र में आपसे दोबारा साइन इन करने को कह सकता है। अपने Claude खाते का उपयोग करें।",

  // Clauseo's document tools, one line under the plans (new; provisional). Never named by what they're made of
  "Also downloads Clauseo's document tools ({size}).":
    "Clauseo के दस्तावेज़ टूल भी डाउनलोड होते हैं ({size})।",
  "Downloading document tools… {done} of {size}": "दस्तावेज़ टूल डाउनलोड हो रहे हैं… {size} में से {done}",
  // their unpack and first run, every byte in (UC-660; the app track's words, hi: later)
  "Setting up the document tools…": null,
  "Clauseo couldn't download its document tools.": "Clauseo अपने दस्तावेज़ टूल डाउनलोड नहीं कर सका।",
  "Windows blocked Clauseo's document tools on this computer.":
    "Windows ने इस कंप्यूटर पर Clauseo के दस्तावेज़ टूल रोक दिए।",

  // the permissions step at the head of the first screen (issue 5, UC-1150 to UC-1166; the app track's words,
  // provisional, hi: later): what macOS asks for, asked here first, and Windows' Controlled folder access
  "Before your first matter": null,
  "Answer here first, so macOS doesn't stop you in the middle of a matter.": null,
  "Your Documents folder": null,
  "Your matters' papers live here. Clauseo, Claude and ChatGPT read them and save their work beside them.":
    null,
  "Your keychain": null,
  "Clauseo keeps your sign-ins and connector passwords in your Mac's keychain. macOS asks once for each and wants your Mac's login password. Choose Always Allow. Plain Allow means macOS asks again next time.":
    null,
  Allow: null,
  "macOS is asking. Choose Allow in its window.": null,
  "macOS is asking. Type your Mac's login password, then choose Always Allow.": null,
  Allowed: null,
  "Not allowed.": null,
  "You can allow it in {path}.": null,
  "Open System Settings": null,
  "You can allow it in Settings, under Privacy, at any time.": null,
  "Clauseo couldn't ask macOS. Try again.": null,
  "Windows is blocking your matters folder": null,
  "Windows blocks the assistants' tools, such as PowerShell, from saving in protected folders. Choose a matters folder outside Documents, Desktop and Pictures, or ask your IT team.":
    null,
  "Windows may be blocking your matters folder": null,
  "Choose a matters folder outside Documents": null,
  "Check again": null,
  // #1303 (UC-1275 to UC-1279): the band's heading over the room for a lawyer who has matters, the refused keychain
  // row's plain line in place of the desktop's raw note, and Windows Security's path introduced
  "Before you go on": null,
  // UC-1550 (provisional, RC A4): the band's one quiet line when the desktop lists Documents not asked again after an
  // update, on a Mac that answered it before
  "Clauseo was updated. Allow it to open your Documents folder again.":
    "Clauseo अपडेट हुआ है। इसे आपका Documents फ़ोल्डर फिर से खोलने की अनुमति दें।",
  "Without it, Clauseo can't read your saved sign-ins for connectors.": null,
  "Without it, Clauseo can't read your saved sign-ins for Claude or ChatGPT.": null,
  "Without it, Clauseo can't read your saved sign-ins for Claude or ChatGPT, or for connectors.":
    null,
  "In Windows Security: {path}": null,
  // the refused keychain row's line where the lawyer's own connectors' keys are among the items (UC-1287 to UC-1290)
  "Without it, Clauseo can't read the saved keys for your own connectors.": null,
  "Without it, Clauseo can't read your saved sign-ins for Claude or ChatGPT, or the saved keys for your own connectors.":
    null,
  "Without it, Clauseo can't read your saved sign-ins for connectors, or the saved keys for your own connectors.":
    null,
  "Without it, Clauseo can't read your saved sign-ins for Claude, ChatGPT and connectors, or the saved keys for your own connectors.":
    null,
  // UC-1440, UC-1441 (provisional): an unreadable key, a signed-out Case Law or a tool's missing key, while the
  // keychain row waits or was refused (keychain-pointer.tsx), with the row's own Allow
  "Clauseo needs your Mac's permission to read its saved keys.":
    "Clauseo को अपनी सहेजी गई कुंजियाँ पढ़ने के लिए आपके Mac की अनुमति चाहिए।",
  "Needs your Mac's permission": "आपके Mac की अनुमति चाहिए",
  // UC-1442 (provisional): the keychain row still would ask right after an Ask (macOS's plain Allow lets one read through)
  "macOS allowed it once only. Press Allow again and choose Always Allow.":
    "macOS ने इसे केवल एक बार अनुमति दी। फिर से Allow दबाएँ और Always Allow चुनें।",
  // UC-1501 to UC-1505 (provisional): a plan the lawyer has, its install redone after an update of Clauseo
  // (app/src/store/agent-update.ts), on the install line in Plans, the first screen and the composer; never "Add"
  "Updating {name}… {done} MB of {total} MB": "{name} अपडेट हो रहा है… {total} MB में से {done} MB",
  "Updating {name}…": "{name} अपडेट हो रहा है…",
  "{name}'s update is paused.": "{name} का अपडेट रुका हुआ है।",
  "{name}'s update stopped.": "{name} का अपडेट बीच में रुक गया।",
  "{name}'s update couldn't be downloaded.": "{name} का अपडेट डाउनलोड नहीं हो सका।",
  // UC-1506 to UC-1508 (provisional): the update not started yet (Missing with installed_before), with Download
  "{name} needs its new version, about {mb} MB.": "{name} को अपना नया संस्करण चाहिए, लगभग {mb} MB।",
  "{name} needs its new version.": "{name} को अपना नया संस्करण चाहिए।",
  Download: "डाउनलोड करें",
} satisfies Hindi<string>;

/** Every string, English and Hindi (null: hi: later), for the tests that hold the voice and the copy review. */
export const firstLaunchWords: Hindi<keyof typeof HI> = HI;
