// What the room's thread reads from the view store beyond one entry or one text: whether a turn runs, whether an answer
// is the one still arriving, and the latest question's words. Each pick returns a boolean or a string, so a flush that
// leaves its answer the same renders nothing.
import { useCallback, useState } from "react";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import { entryId, textKey, type StreamView } from "../../store/view.ts";
import type {
  EntryId,
  StreamId,
  ThreadStatus,
  TurnId,
  UserMessageEntry,
} from "../../transport/protocol.ts";
import { exchangesPick, latestAsked } from "./exchanges.ts";

/** Whether the thread's status says a turn runs or is about to: Stop stands in Ask's place. */
export function turnRunning(status: ThreadStatus | undefined): boolean {
  if (status === undefined) return false;
  if (typeof status === "string")
    return status === "starting" || status === "running" || status === "needs_you";
  return status.queued !== undefined;
}

const statusOf = (v: StreamView | undefined) => v?.header?.header?.status;

/** Busy: the status says a turn runs, or the prompt sent last has neither its step nor its limit stop in the view. */
export function useBusy(stream: StreamId | null, sent: TurnId | null): boolean {
  const pick = useCallback(
    (v: StreamView | undefined) => {
      if (turnRunning(statusOf(v))) return true;
      if (sent === null) return false;
      return (
        v === undefined ||
        !(v.entries.has(entryId("step", sent)) || v.entries.has(entryId("limit_stop", sent)))
      );
    },
    [sent],
  );
  return useStreamPart(useViewStore(), stream, pick);
}

/** Whether the answer `id` is still arriving: a turn runs and the agent's latest output is this answer. A boolean, so
 * a finished answer renders once more (its final render) and then not again. */
export function useStreaming(stream: StreamId, id: EntryId): boolean {
  const pick = useCallback(
    (v: StreamView | undefined) =>
      v !== undefined && turnRunning(statusOf(v)) && v.latestOutput === id,
    [id],
  );
  return useStreamPart(useViewStore(), stream, pick);
}

/** A question's text blocks, by their keys in `texts`, with each block's text as the entry has it. Kept per entry
 * object (an upsert makes a new one), so a flush reads them without building a key. */
type Blocks = { readonly keys: readonly string[]; readonly own: readonly string[] };
const blocksByEntry = new WeakMap<UserMessageEntry, Blocks>();

function blocksOf(id: EntryId, entry: UserMessageEntry): Blocks {
  let blocks = blocksByEntry.get(entry);
  if (blocks === undefined) {
    const keys: string[] = [];
    const own: string[] = [];
    entry.content.forEach((block, i) => {
      if (block.text === undefined) return;
      keys.push(textKey(id, i));
      own.push(block.text.text);
    });
    blocks = { keys, own };
    blocksByEntry.set(entry, blocks);
  }
  return blocks;
}

/** A question's words: its text blocks, as `texts` holds them while they arrive (or as the entry has them, when no
 * view does), one paragraph each. A question of one block, the usual one, is that block's own string: no copy, the
 * same identity while it is unchanged. */
export function questionText(
  id: EntryId,
  entry: UserMessageEntry,
  texts: ReadonlyMap<string, string> | undefined,
): string {
  const { keys, own } = blocksOf(id, entry);
  if (keys.length === 1) return texts?.get(keys[0]!) ?? own[0]!;
  return keys.map((key, i) => texts?.get(key) ?? own[i]!).join("\n\n");
}

/** A question's words from the store, by its id. */
export function useQuestionText(stream: StreamId, id: EntryId, entry: UserMessageEntry): string {
  const pick = useCallback(
    (v: StreamView | undefined) => questionText(id, entry, v?.texts),
    [id, entry],
  );
  return useStreamPart(useViewStore(), stream, pick);
}

/** The latest question's words, for the question pinned over the thread; "" before the first. The question that
 * heads the newest exchange (exchanges.ts): never a queued or removed message, nor a steer taken into an answer. */
export function useLatestQuestion(stream: StreamId | null): string {
  const [shapeOf] = useState(exchangesPick);
  const pick = useCallback(
    (v: StreamView | undefined) => {
      const id = v === undefined ? null : latestAsked(shapeOf(v));
      const entry = id === null ? undefined : v!.entries.get(id)?.user_message;
      return id === null || entry === undefined ? "" : questionText(id, entry, v!.texts);
    },
    [shapeOf],
  );
  return useStreamPart(useViewStore(), stream, pick);
}

const pickHasOlder = (v: StreamView | undefined) => v?.hasOlder ?? false;

/** Whether the view has entries older than its window. */
export const useHasOlder = (stream: StreamId | null): boolean =>
  useStreamPart(useViewStore(), stream, pickHasOlder);
