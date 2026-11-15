// The Hindi of words-refusals.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-refusals.ts imports this file's type alone. hi: later: null
// shows the English until the Hindi comes.

export const HI = {
  // the safety check's reason, read from its tag: what the action looked like
  "Claude's built-in safety check stopped this: it looked like reaching files or systems Claude wasn't asked to use. Nothing was changed, and Claude carries on another way.":
    null,
  "Claude's built-in safety check stopped this: it looked like sending matter information somewhere outside. Nothing was changed, and Claude carries on another way.":
    null,
  "Claude's built-in safety check stopped this: it looked like a search for passwords or sign-in keys. Nothing was changed, and Claude carries on another way.":
    null,
  "Claude's built-in safety check stopped this: it looked like deleting or overwriting work in a way that can't be undone. Nothing was changed, and Claude carries on another way.":
    null,
  "Claude's built-in safety check stopped this: it looked like getting around the checks on Claude's work. Nothing was changed, and Claude carries on another way.":
    null,
  "Claude's built-in safety check stopped this: it looked like changing how Claude itself is set up. Nothing was changed, and Claude carries on another way.":
    null,
  "Claude's built-in safety check stopped this: it looked like changing something outside this matter. Nothing was changed, and Claude carries on another way.":
    null,
  "Claude's built-in safety check stopped this: it looked like following instructions hidden in a document or web page. Nothing was changed, and Claude carries on another way.":
    null,
  "Claude's built-in safety check stopped this: it looked like running code from a source that isn't trusted. Nothing was changed, and Claude carries on another way.":
    null,
  "Claude's built-in safety check stopped this: it looked like spending money or acting for you outside Clauseo. Nothing was changed, and Claude carries on another way.":
    null,
  // a reason it gave in words this list doesn't know
  "Claude's built-in safety check stopped this. Nothing was changed, and Claude carries on another way.":
    null,
  // the check gave no verdict (it was busy, or couldn't read this), so the action didn't run
  "Claude's built-in safety check couldn't look at this just now, so it didn't run. Nothing was changed.":
    null,
  // a rule Clauseo set for the matter: its own app data, another matter, a matter's .clauseo folder
  "This tried to reach somewhere Clauseo keeps off limits here, such as Clauseo's own app data or another matter. Nothing was changed.":
    null,
} as const;

/** Every refusal line, English and Hindi (null: hi: later), for the voice test and the copy review. */
export const refusalWords: Readonly<Record<string, string | null>> = HI;
