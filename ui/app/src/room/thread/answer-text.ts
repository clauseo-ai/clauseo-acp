// An answer's words as Copy puts them on the clipboard (answer-actions.tsx, D-026): pure, so a test reads it alone.
import { textKey, type StreamView } from "../../store/view.ts";
import type { EntryId, ViewEntry } from "../../transport/protocol.ts";

const TEXT = "agent_text:";

/** The answer's words, in order, from an older page or the view: each of its texts that isn't a thought. */
export function answerText(
  ids: readonly EntryId[],
  view: StreamView | undefined,
  paged: ReadonlyMap<EntryId, ViewEntry>,
): string {
  const out: string[] = [];
  for (const id of ids) {
    if (!id.startsWith(TEXT)) continue;
    const page = paged.get(id)?.agent_text;
    const entry = page ?? view?.entries.get(id)?.agent_text;
    if (entry === undefined || entry.kind === "thought") continue;
    const text = page !== undefined ? page.text : (view?.texts.get(textKey(id)) ?? entry.text);
    if (text.trim() !== "") out.push(text.trim());
  }
  return out.join("\n\n");
}
