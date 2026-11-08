// The Hindi of words-denials.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-denials.ts imports this file's type alone. Provisional (R18), the
// app's own terms: मामला, अनुमति दें, अभी नहीं, फ़ोल्डर, वेबसाइट; "Not now" and "Not allowed." say what the other files say.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // a folder or a file beyond the matter; the path stands on its own line, in <bdi>
  "Allow access to this folder?": "इस फ़ोल्डर तक पहुँच की अनुमति दें?",
  "Allow access to this file?": "इस फ़ाइल तक पहुँच की अनुमति दें?",
  "Clauseo will be able to read it for this matter only.": "Clauseo इसे केवल इस मामले के लिए पढ़ सकेगा।",
  "This is in another matter: {matter}.": "यह दूसरे मामले में है: {matter}।",
  "Allow access": "पहुँच की अनुमति दें",
  "Not now": "अभी नहीं",
  // a site the proxy refused; the exact host on its own line, in <bdi>
  "Allow this site?": "इस वेबसाइट की अनुमति दें?",
  "This address is shared by many sites. Allowing it allows all of them.":
    "यह पता कई वेबसाइटों का साझा पता है। इसकी अनुमति देने से उन सभी की अनुमति मिल जाती है।",
  "Allow this site": "इस वेबसाइट की अनुमति दें",
  // no card: the host never widens it
  "Clauseo kept this folder closed. It can't be allowed for this matter.":
    "Clauseo ने यह फ़ोल्डर बंद रखा। इस मामले के लिए इसकी अनुमति नहीं दी जा सकती।",
  "Clauseo kept this file closed. It can't be allowed for this matter.":
    "Clauseo ने यह फ़ाइल बंद रखी। इस मामले के लिए इसकी अनुमति नहीं दी जा सकती।",
  "Clauseo kept this site closed. It can't be allowed for this matter.":
    "Clauseo ने यह वेबसाइट बंद रखी। इस मामले के लिए इसकी अनुमति नहीं दी जा सकती।",
  // a Windows tool call that reached into another matter: no card, the way to share that matter named instead;
  // {name} is the matter's AI tool (Claude, Codex, or its own name), else "The AI tool"
  "{name} tried to open a file from another matter, {matter}. Clauseo kept it closed. To share that matter's files, choose Use files from another matter.":
    "{name} ने दूसरे मामले, {matter}, की एक फ़ाइल खोलने की कोशिश की। Clauseo ने उसे बंद रखा। उस मामले की फ़ाइलें साझा करने के लिए ‘दूसरे मामले की फ़ाइलें इस्तेमाल करें’ चुनें।",
  "The AI tool": "AI टूल",
  "Your network needs a sign-in Clauseo can't give.":
    "आपके नेटवर्क को ऐसा साइन इन चाहिए जो Clauseo नहीं दे सकता।",
  // the card's states, each after the path or host and a middle dot
  "Waiting for your answer in the window Clauseo opened…":
    "Clauseo ने जो विंडो खोली है, उसमें आपके उत्तर की प्रतीक्षा है…",
  "Allowed.": "अनुमति दी गई।",
  "Going on with the folder…": "फ़ोल्डर के साथ आगे बढ़ रहा है…",
  "Going on with the file…": "फ़ाइल के साथ आगे बढ़ रहा है…",
  "Not allowed.": "अनुमति नहीं दी गई।",
  "Not now.": "अभी नहीं।",
  "This request has ended.": "यह अनुरोध समाप्त हो गया है।",
  // the answer paused after too many refusals, {n} from 3
  "Clauseo stopped after {n} blocked attempts outside this matter.":
    "इस मामले के बाहर {n} कोशिशें रोकी गईं, इसलिए Clauseo रुक गया।",
} as const;

/** Every key, for the banned-words screen in the tests. */
export const denialWords: Hindi<keyof typeof HI> = HI;
