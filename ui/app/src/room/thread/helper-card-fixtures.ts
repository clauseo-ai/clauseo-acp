// Shared by the helper card's tests (helper-card.test.tsx, helper-card-words.test.tsx): the room
// mounted over a thread with a question and helper entries, and the view a pick reads. Not shipped.
import { header, reset, userMessage } from "../../testing/stream-script.ts";
import { entryIdOf, type StreamView } from "../../store/view.ts";
import type { ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

/** The mounted room, set by `room`; read through the live binding. */
export let h: Harness | null = null;

/** For each file's afterEach: unmount the room. */
export async function cleanup() {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
}

const ASKED = userMessage(
  5,
  1,
  "Trace the title to Plot 14 through the deeds and tell me where it breaks.",
);
export const TASK =
  "Read every sale deed in the matter and trace the chain of title to Plot 14, with dates.";

/** The room over the question and `entries`, its header saying `status` and `agent`, then
 * whatever `more` sets on it. */
export async function room(
  entries: ViewEntry[],
  status: "idle" | "running" = "idle",
  agent: "claude-acp" | "codex-acp" = "claude-acp",
  more: (head: ViewEntry) => void = () => {},
) {
  const mounted = await mountRoom();
  h = mounted;
  const head = header(status);
  head.header!.agent = agent;
  more(head);
  mounted.deliver([reset([ASKED, ...entries], head)]);
  await mounted.settle();
  return mounted;
}

export const cards = () => h!.all("helper-card");
export const text = (el: Element | null | undefined) => el?.textContent ?? "";

/** A stream view over `entries` for a pick, its header saying `status`. */
export const viewOf = (entries: ViewEntry[], status: "running" | "idle" = "running"): StreamView =>
  ({
    header: header(status),
    ids: entries.map(entryIdOf),
    entries: new Map(entries.map((e) => [entryIdOf(e), e])),
  }) as unknown as StreamView;
