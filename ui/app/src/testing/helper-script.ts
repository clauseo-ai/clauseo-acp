// Stream-script entries for helpers (C2's CONTRACT.md §3 and §6), for the room's tests and the
// review pages. Not shipped: only tests and review import it.
import type { ToolCallEntry, ToolCallStatus, ToolKind, ViewEntry } from "../transport/protocol.ts";
import {
  helperStepViewEntry,
  helperTextViewEntry,
  helperViewEntry,
  type HelperEntry,
} from "../room/thread/helper-entries.ts";
import { TURN, uuid } from "./stream-script.ts";

/** Helper `n`'s id. */
export const helperId = (n: number): string => uuid(n, 11);

/** Helper `n`, started at `order` (its time `order` seconds in), with `over`. */
export const helper = (order: number, n: number, over: Partial<HelperEntry> = {}): ViewEntry =>
  helperViewEntry({
    order,
    helper: helperId(n),
    turn: TURN,
    name: "paper-reader",
    started_at: order * 1000,
    ...over,
  });

/** A run of helper `of`'s words. */
export const helperText = (order: number, n: number, of: number, text: string): ViewEntry =>
  helperTextViewEntry({ order, event: uuid(n, 12), turn: TURN, helper: helperId(of), text });

/** Step `n` of helper `of` (null: the lead's own), reading `name`, or of `kind` with `title`. */
export function helperStep(
  order: number,
  n: number,
  of: number | null,
  status: ToolCallStatus,
  title = `/matter/Sale deed ${n}.pdf`,
  kind: ToolKind = "read",
): ViewEntry {
  const entry: ToolCallEntry = {
    order,
    tool_call: uuid(n, 7),
    turn: TURN,
    started_at: order * 1000,
    title,
    kind,
    status,
    locations: kind === "read" ? [{ inside: { path: title.replace(/^\/matter\//, "") } }] : [],
    content: [],
    ...(status === "completed" || status === "failed" ? { finished_at: order * 1000 + 500 } : {}),
  };
  return helperStepViewEntry(entry, of === null ? null : helperId(of));
}

/** Claude's Agent step `n` that launches a helper, its title the agent's short label. */
export function agentCall(
  order: number,
  n: number,
  status: ToolCallStatus,
  label: string,
): ViewEntry {
  return {
    tool_call: {
      order,
      tool_call: uuid(n, 7),
      turn: TURN,
      started_at: order * 1000,
      title: label,
      kind: "think",
      tool_name: "Agent",
      status,
      locations: [],
      content: [],
    },
  };
}

/** The tool call id of step `n`, for a helper's `call`. */
export const callId = (n: number): string => uuid(n, 7);

/** What Claude's helper reported when the agent cut it at its turn limit with no report, as C2's
 * recorded run L4 has it (crates/host/src/actor/turn/tests/recorded/claude-turn-limit.jsonl). */
export const LIMIT_SUMMARY =
  "[Agent stopped at its turn limit — the output below is partial]\n\nThe subagent ended without " +
  "delivering a report through SubagentHandback, so no report was delivered. Its unsent text is " +
  "not shown. Send the agent a message (SendMessage) to ask it to deliver its report.";
