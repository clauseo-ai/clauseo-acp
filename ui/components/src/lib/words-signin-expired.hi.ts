// The Hindi of words-signin-expired.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-signin-expired.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // UD2-21 to UD2-23: the plan's sign-in expired
  "Your {name} sign-in has expired. Your question is kept.":
    "आपका {name} साइन इन समाप्त हो गया है। आपका प्रश्न रखा गया है।",
  "Sign in again opens {name}'s own sign-in in your browser. Only the sign-in repeats: this matter, its papers and your settings stay as they are.":
    "दोबारा साइन इन करने पर आपके ब्राउज़र में {name} का अपना साइन इन खुलता है। केवल साइन इन दोहराया जाता है। यह मामला, इसके दस्तावेज़ और आपकी सेटिंग्स जैसे हैं वैसे रहते हैं।",
  "Sign in again": "दोबारा साइन इन करें",
  // UD2-24: waiting in the browser
  "Finish signing in in your browser. Your question is asked once you're back.":
    "अपने ब्राउज़र में साइन इन पूरा करें। आपके लौटते ही आपका प्रश्न पूछा जाएगा।",
  // UD2-25 to UD2-27: signed in again
  "Signed in again at {time} as {email}, {plan}.":
    "{time} बजे {email} के रूप में दोबारा साइन इन हुआ, {plan}।",
  "Signed in again at {time} as {email}.": "{time} बजे {email} के रूप में दोबारा साइन इन हुआ।",
  "Signed in again at {time}.": "{time} बजे दोबारा साइन इन हुआ।",
  // UD2-28: under a line of the gate's own
  "Your question is kept.": "आपका प्रश्न रखा गया है।",
  // UD2-29 to UD2-31: a key or the firm's account refused
  "{name} didn't accept the API key. Your question is kept.":
    "{name} ने API key स्वीकार नहीं की। आपका प्रश्न रखा गया है।",
  "To change it, open Settings, then Plans.": "इसे बदलने के लिए सेटिंग्स खोलें, फिर प्लान।",
  "Your firm's account didn't accept the question. Your question is kept.":
    "आपकी फ़र्म के खाते ने प्रश्न स्वीकार नहीं किया। आपका प्रश्न रखा गया है।",
} satisfies Hindi<string>;
