// Where the first read stands, from the thread's view and its marked turns (marks.ts), and the line's words: pure, for
// first-read-line.tsx and its tests.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { count } from "@clauseo/components/lib/money.ts";
import { baseName, isolate } from "@clauseo/components/lib/words-core.ts";
import { frd } from "@clauseo/components/lib/words-first-read.ts";
import type { StreamView } from "../../store/view.ts";
import type { ToolCallEntry, TurnId } from "../../transport/protocol.ts";

export type ReadState =
  | { kind: "reading"; title: string | null }
  | { kind: "read" }
  | { kind: "stopped" };

const USER = "user_message:";
const STEP = "step:";
const TOOL = "tool_call:";

/** The file a read tool call names: its first location in the matter, else its title. */
function fileOf(tool: ToolCallEntry): string | null {
  for (const at of tool.locations) {
    if (typeof at === "object" && at.inside !== undefined) return baseName(at.inside.path);
  }
  return tool.title.trim() === "" ? null : baseName(tool.title.trim());
}

/** Where the read stands, when the thread's latest question is a first-read turn; null otherwise, or when its turn
 * failed (U10's line says that). */
export function readStateOf(
  view: StreamView | undefined,
  turns: readonly TurnId[],
): ReadState | null {
  if (view === undefined || turns.length === 0) return null;
  let turn: TurnId | null = null;
  let at = -1;
  for (let i = view.ids.length - 1; i >= 0; i -= 1) {
    const id = view.ids[i]!;
    if (!id.startsWith(USER)) continue;
    const m = view.entries.get(id)?.user_message;
    if (m === undefined) continue;
    turn = m.turn;
    at = i;
    break;
  }
  if (turn === null || !turns.includes(turn)) return null;
  let title: string | null = null;
  for (let i = view.ids.length - 1; i > at; i -= 1) {
    const id = view.ids[i]!;
    const entry = view.entries.get(id);
    if (id.startsWith(STEP) && entry?.step?.turn === turn) {
      const stop = entry.step.stop;
      if (stop === "cancelled") return { kind: "stopped" };
      if (stop === "failed" || stop === "at_limit") return null;
      return { kind: "read" };
    }
    const tool = entry?.tool_call;
    if (
      title === null &&
      id.startsWith(TOOL) &&
      tool !== undefined &&
      tool.turn === turn &&
      tool.kind === "read"
    )
      title = fileOf(tool);
  }
  return { kind: "reading", title };
}

/** The line's words for a state and `n` files. */
export function readLine(lang: Lang, state: ReadState, n: number): string {
  const one = n === 1;
  switch (state.kind) {
    case "reading":
      if (state.title === null)
        return one
          ? frd(lang, "Reading 1 file.")
          : frd(lang, "Reading {n} files.", { n: count(n) });
      return one
        ? frd(lang, "Reading 1 file. Now {title}.", { title: isolate(state.title) })
        : frd(lang, "Reading {n} files. Now {title}.", {
            n: count(n),
            title: isolate(state.title),
          });
    case "read":
      return one ? frd(lang, "Read 1 file.") : frd(lang, "Read {n} files.", { n: count(n) });
    case "stopped":
      return frd(lang, "Clauseo stopped. The answer so far is kept.");
  }
}
