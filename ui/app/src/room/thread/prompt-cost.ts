// What a question costs against the host's cap on a prompt's text (MAX_PROMPT_TEXT_BYTES): its text counted as the host
// counts it, the UTF-8 bytes of the text as a JSON string, quotes left out (crates/protocol's escaped_len,
// `serde_json::to_string(text).len() - 2`). Measured once per send, never per keystroke: one pass over the UTF-16 code
// units, no copy of the text. The shared cases in crates/protocol/tests/support/escaped-len-cases.json hold both counts
// to the same answers (prompt-cost.test.ts).

/** The host's MAX_PROMPT_TEXT_BYTES (6 MiB), copied: the generated bindings.ts imports Tauri, which the room never
 * loads outside src/transport. prompt-cost.test.ts fails when the two differ. */
export const MAX_PROMPT_TEXT_BYTES = 6 * 1024 * 1024;

/** The bytes `text` takes as a JSON string's contents: `"`, `\` and \b \t \n \f \r take 2, the other code units below
 * 0x20 take 6 (`\u00XX`), DEL and everything from U+0080 their UTF-8 bytes (serde_json escapes no more), a surrogate
 * pair 4. A lone surrogate, which a textarea can hold but serde never sees (the webview sends it as U+FFFD), counts 3,
 * as U+FFFD would. */
export function escapedLen(text: string): number {
  let bytes = 0;
  const n = text.length;
  for (let i = 0; i < n; i++) {
    const c = text.charCodeAt(i);
    if (c < 0x20) {
      // \b \t \n \f \r: their short escape; every other control: \u00XX
      bytes += c === 0x08 || c === 0x09 || c === 0x0a || c === 0x0c || c === 0x0d ? 2 : 6;
    } else if (c < 0x80) {
      bytes += c === 0x22 || c === 0x5c ? 2 : 1;
    } else if (c < 0x800) {
      bytes += 2;
    } else if (c >= 0xd800 && c <= 0xdbff) {
      const next = i + 1 < n ? text.charCodeAt(i + 1) : 0;
      if (next >= 0xdc00 && next <= 0xdfff) {
        bytes += 4;
        i++;
      } else bytes += 3;
    } else {
      // the rest of the BMP, a lone low surrogate included
      bytes += 3;
    }
  }
  return bytes;
}

/** Whether a question's text is over the host's cap, so a send of it would be refused with `prompt.too_long`. The
 * composer's question is the prompt's one Text block (paper-mentions.ts's promptContent). */
export const tooLong = (text: string): boolean => escapedLen(text) > MAX_PROMPT_TEXT_BYTES;
