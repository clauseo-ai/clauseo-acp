// The Hindi of words-breaks.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-breaks.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // the working line, when Claude stopped mid-answer: its time is when the host saw Claude's process end
  "The answer stopped at {time}. Your question and the answer so far are kept.":
    "उत्तर {time} बजे रुक गया। आपका प्रश्न और अब तक का उत्तर रखे गए हैं।",
  // the mark in the thread, under the answer so far: today, or on another day, as "Continues on {day} at {time}" says
  "The answer stopped here at {time}": "उत्तर {time} बजे यहाँ रुका",
  "The answer stopped here on {day} at {time}": "उत्तर {day} को {time} बजे यहाँ रुका",
  // Restart's details, only once the host restarts (H10), and only for agent.exited
  "The answer stopped with no reason given. Restart asks your question once more. The answer so far stays above.":
    "उत्तर बिना किसी कारण के रुक गया। फिर से शुरू करने पर आपका प्रश्न एक बार फिर पूछा जाता है। अब तक का उत्तर ऊपर बना रहता है।",
  // a stream that failed: the thread stopped coming in, said before its code's sentence
  "This conversation stopped updating in this window.": "यह बातचीत इस विंडो में अपडेट होना बंद हो गई।",
  // a code's sentence that says to try again, for a stream whose failure another try won't mend (no Try again shows)
  "The computer's disk is full. Free some space.": "कंप्यूटर की डिस्क भर गई है। कुछ जगह खाली करें।",
} as const;

/** Every key, for the banned-words screen in the tests. */
export const breakWords: Hindi<keyof typeof HI> = HI;
