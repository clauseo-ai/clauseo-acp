// The Hindi of words-signin-expired.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-signin-expired.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // UD2-21 to UD2-23: the plan's sign-in expired
  "Your {name} sign-in has expired. Your question is kept.": null, // hi: later
  "Sign in again opens {name}'s own sign-in in your browser. Only the sign-in repeats: this matter, its papers and your settings stay as they are.":
    null, // hi: later
  "Sign in again": null, // hi: later
  // UD2-24: waiting in the browser
  "Finish signing in in your browser. Your question is asked once you're back.": null, // hi: later
  // UD2-25 to UD2-27: signed in again
  "Signed in again at {time} as {email}, {plan}.": null, // hi: later
  "Signed in again at {time} as {email}.": null, // hi: later
  "Signed in again at {time}.": null, // hi: later
  // UD2-28: under a line of the gate's own
  "Your question is kept.": null, // hi: later
  // UD2-29 to UD2-31: a key or the firm's account refused
  "{name} didn't accept the API key. Your question is kept.": null, // hi: later
  "To change it, open Settings, then Plans.": null, // hi: later
  "Your firm's account didn't accept the question. Your question is kept.": null, // hi: later
} satisfies Hindi<string>;
