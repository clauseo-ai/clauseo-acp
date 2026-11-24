// The helper entries of a thread's view (C2: clauseo-stagec/issue13/CONTRACT.md §3) and the one
// place that reads them. Every other file reads a view entry's helper parts through `helperPartOf`,
// so the day C2's bindings land, only this file changes. TODO(C2's PR #1276, host-helper-events):
// when it merges, replace the local types below with the generated ones: export type { HelperEntry,
// HelperId, HelperOutcome, HelperTextEntry } from "../../transport/protocol.ts"; and read
// `entry.helper`, `entry.helper_text` and `entry.tool_call?.helper` in `helperPartOf` without the
// cast.
import type {
  EntryId,
  EventId,
  Seq,
  TextOrBlob,
  Timestamp,
  ToolCallId,
  TurnId,
  ViewEntry,
} from "../../transport/protocol.ts";

/** C2: Clauseo's id for a helper. */
export type HelperId = string;

/** C2: how a helper ended. Open: a value from a newer Clauseo reads as `unknown`. */
export type HelperOutcome = "done" | "failed" | "stopped" | "unknown";

/** C2's `HelperEntry`, as specta will write it. Entry id `helper:<helper>`. */
export type HelperEntry = {
  order: Seq;
  helper: HelperId;
  turn: TurnId;
  parent?: HelperId;
  call?: ToolCallId;
  name: string;
  label?: string;
  task?: TextOrBlob;
  model?: string;
  outcome?: HelperOutcome;
  summary?: TextOrBlob;
  started_at: Timestamp;
  finished_at?: Timestamp;
};

/** C2's `HelperTextEntry`. Entry id `helper_text:<event>`. */
export type HelperTextEntry = {
  order: Seq;
  event: EventId;
  turn: TurnId;
  helper: HelperId;
  text: string;
};

/** What a view entry is to the helpers: a helper, a run of a helper's words, a step (`helper` its
 * helper's id, or null for the lead's own), or nothing of theirs. */
export type HelperPart =
  | { kind: "helper"; entry: HelperEntry }
  | { kind: "text"; entry: HelperTextEntry }
  | { kind: "step"; helper: HelperId | null }
  | null;

type Wire = { helper?: unknown; helper_text?: unknown; tool_call?: { helper?: unknown } };

/** The one reader of the helper parts of a view entry. */
export function helperPartOf(entry: ViewEntry | undefined): HelperPart {
  if (entry === undefined) return null;
  const w = entry as unknown as Wire;
  if (typeof w.helper === "object" && w.helper !== null)
    return { kind: "helper", entry: w.helper as HelperEntry };
  if (typeof w.helper_text === "object" && w.helper_text !== null)
    return { kind: "text", entry: w.helper_text as HelperTextEntry };
  if (w.tool_call !== undefined)
    return {
      kind: "step",
      helper: typeof w.tool_call.helper === "string" ? w.tool_call.helper : null,
    };
  return null;
}

/** The turn of a helper or of its words, for the exchange it joins; undefined for anything else. */
export function helperTurnOf(entry: ViewEntry): TurnId | undefined {
  const part = helperPartOf(entry);
  return part?.kind === "helper" || part?.kind === "text" ? part.entry.turn : undefined;
}

/** C2's follow-up to #1276 (not wired yet): the helper entries that travel beside a view window or
 * an older page, as its optional `helpers` list, merged by id into its entries in `order`, an entry
 * the page already holds kept. The day the list lands, the window's reset and older.ts's page each
 * pass through this. */
export function withHelpersBeside(
  entries: readonly ViewEntry[],
  page: { helpers?: readonly HelperEntry[] | null },
): readonly ViewEntry[] {
  const beside = page.helpers ?? [];
  if (beside.length === 0) return entries;
  const have = new Set<HelperId>();
  for (const e of entries) {
    const part = helperPartOf(e);
    if (part?.kind === "helper") have.add(part.entry.helper);
  }
  const add = beside.filter((h) => !have.has(h.helper)).map(helperViewEntry);
  if (add.length === 0) return entries;
  const orderOf = (e: ViewEntry) =>
    (Object.values(e)[0] as { order: number } | undefined)?.order ?? 0;
  return [...entries, ...add].sort((a, b) => orderOf(a) - orderOf(b));
}

/** The entry id of a helper, as the store builds it. */
export const helperEntryId = (helper: HelperId): EntryId => `helper:${helper}`;

/** A helper or its words as a view entry, for fixtures and the review pages. */
export function helperViewEntry(entry: HelperEntry): ViewEntry {
  return { helper: entry } as unknown as ViewEntry;
}
export function helperTextViewEntry(entry: HelperTextEntry): ViewEntry {
  return { helper_text: entry } as unknown as ViewEntry;
}
