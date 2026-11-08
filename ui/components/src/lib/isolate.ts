// A value from outside Clauseo (a path, a host, a matter's name, the agent's words) made safe to show among the app's
// own: its control characters (Unicode Cc: a line break, a tab and the rest) and its bidi marks, embeddings,
// overrides and isolates (U+061C, U+200E, U+200F, U+202A-U+202E, U+2066-U+2069) go, then the rest is wrapped in FIRST
// STRONG ISOLATE (U+2068) and POP DIRECTIONAL ISOLATE (U+2069). So a right-to-left name can't reorder the words around
// it, close the isolate early, or fake a line. The same rule as the native dialogs' `isolate`
// (crates/desktop/src/dialogs/words.rs), whose `char::is_control` is Unicode's Cc.
const DROPPED = /[\p{Cc}\u061C\u200E\u200F\u202A-\u202E\u2066-\u2069]/gu;

/** `text` without control or bidi formatting characters, wrapped in FSI … PDI. */
export function isolate(text: string): string {
  return `\u2068${text.replace(DROPPED, "")}\u2069`;
}
