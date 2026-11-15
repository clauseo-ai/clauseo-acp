// Letting a conversation read another matter's files (Rohan, 11-08): which other matters the open thread may read,
// from its header's `read_matters`, the app's matter names by id, and the two commands. The host keeps the choice in
// the thread's own log and answers Done; the header then lists the matter, or no longer does, so nothing here holds
// state of its own. The room gives the names (room.tsx, from its matters list) to the thread and the composer; the
// rail reads its own view.
import { createContext, createElement, use, useMemo, type ReactNode } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { t } from "@clauseo/components/lib/words.ts";
import { useStreamPart } from "../store/hooks.ts";
import type { ViewStore } from "../store/store.ts";
import type { StreamView } from "../store/view.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { CommandOutcome, MatterId, StreamId, ThreadId } from "../transport/protocol.ts";

/** Each matter's name as the room's list has it now, the scratch matter's in the page's words. */
export type MatterNames = ReadonlyMap<MatterId, string>;

const NO_NAMES: MatterNames = new Map();

export const MatterNamesContext = createContext<MatterNames>(NO_NAMES);

/** The room's matter names, by id; empty outside the room. */
export const useRoomMatterNames = (): MatterNames => use(MatterNamesContext);

/** The matters' names by id, as the room shows them: a folder matter's as filed, the scratch matter's as the room
 * names it. */
export function matterNamesOf(
  matters: readonly { matter: MatterId; name: string; kind: string }[],
  lang: Lang,
): MatterNames {
  return new Map(
    matters.map((m) => [m.matter, m.kind === "scratch" ? t(lang, "Scratch matter") : m.name]),
  );
}

/** Gives the room's matter names (`matters`, in `lang`) to the thread's lines and the composer's pills. */
export function MatterNamesFrom({
  matters,
  lang,
  children,
}: {
  matters: readonly { matter: MatterId; name: string; kind: string }[];
  lang: Lang;
  children: ReactNode;
}): ReactNode {
  const names = useMemo(() => matterNamesOf(matters, lang), [matters, lang]);
  return createElement(MatterNamesContext, { value: names }, children);
}

/** The header's read matters as one string, so a header that changes for anything else re-renders nothing. */
const readKey = (v: StreamView | undefined): string =>
  v?.header?.header?.read_matters?.join("\n") ?? "";

/** The other matters the thread on `stream` may read, oldest first; none for a draft or a header without them. */
export function useReadMatters(store: ViewStore, stream: StreamId | null): readonly MatterId[] {
  const key = useStreamPart(store, stream, readKey);
  return useMemo(() => (key === "" ? [] : key.split("\n")), [key]);
}

/** Lets `thread` read `matter`'s files (allow_matter_read); resolves with the host's Done, rejects with its refusal. */
export const allowRead = (
  transport: Pick<HostTransport, "dispatch">,
  thread: ThreadId,
  matter: MatterId,
): Promise<CommandOutcome> =>
  transport.dispatch({
    command_id: crypto.randomUUID(),
    command: { allow_matter_read: { thread, matter } },
  });

/** Stops letting `thread` read `matter`'s files (withdraw_matter_read). */
export const withdrawRead = (
  transport: Pick<HostTransport, "dispatch">,
  thread: ThreadId,
  matter: MatterId,
): Promise<CommandOutcome> =>
  transport.dispatch({
    command_id: crypto.randomUUID(),
    command: { withdraw_matter_read: { thread, matter } },
  });

/** The matters `thread` (of `own`) could be let read: every other matter in the list not already allowed, by name. */
export function readableOthers(
  names: MatterNames,
  own: MatterId,
  allowed: readonly MatterId[],
): { matter: MatterId; name: string }[] {
  return [...names]
    .filter(([id]) => id !== own && !allowed.includes(id))
    .map(([matter, name]) => ({ matter, name }));
}
