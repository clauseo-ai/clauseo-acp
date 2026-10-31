// Reading the store from React. Each hook picks one part of one stream's view, a part that keeps its identity while it
// is unchanged (an entry object, the id list, a string), so a flush that changes one row renders that row alone. The
// view itself is changed in place, so no hook hands out the whole view.
import { createContext, use, useCallback, useEffect, useSyncExternalStore } from "react";
import type {
  EntryId,
  HostError,
  StreamHeader,
  StreamId,
  ViewEntry,
} from "../transport/protocol.ts";
import type { ViewStore } from "./store.ts";
import type { StreamView } from "./view.ts";

/** The store the window reads; the app provides it. */
export const StoreContext = createContext<ViewStore | null>(null);

export function useViewStore(): ViewStore {
  const store = use(StoreContext);
  if (store === null) throw new Error("no StoreContext above this component");
  return store;
}

const NO_IDS: readonly EntryId[] = [];

/** A part of a stream's view. `pick` must return the same value while its part is unchanged. */
export function useStreamPart<T>(
  store: ViewStore,
  stream: StreamId | null,
  pick: (view: StreamView | undefined) => T,
): T {
  const read = useCallback(
    () => pick(stream === null ? undefined : store.view(stream)),
    [store, stream, pick],
  );
  return useSyncExternalStore(store.subscribe, read, read);
}

/** Holds a stream while the component is mounted. */
export function useWatch(store: ViewStore, stream: StreamId | null): void {
  useEffect(() => (stream === null ? undefined : store.watch(stream)), [store, stream]);
}

const pickIds = (v: StreamView | undefined) => v?.ids ?? NO_IDS;
const pickHeader = (v: StreamView | undefined) => v?.header ?? null;

export const useEntryIds = (store: ViewStore, stream: StreamId | null): readonly EntryId[] =>
  useStreamPart(store, stream, pickIds);

export const useHeader = (store: ViewStore, stream: StreamId | null): StreamHeader | null =>
  useStreamPart(store, stream, pickHeader);

export function useEntry(store: ViewStore, stream: StreamId, id: EntryId): ViewEntry | undefined {
  const pick = useCallback((v: StreamView | undefined) => v?.entries.get(id), [id]);
  return useStreamPart(store, stream, pick);
}

/** A streaming text by its key (`textKey`); "" before it exists. */
export function useText(store: ViewStore, stream: StreamId, key: string): string {
  const pick = useCallback((v: StreamView | undefined) => v?.texts.get(key) ?? "", [key]);
  return useStreamPart(store, stream, pick);
}

/** Why a stream stopped, or null while it runs. */
export function useStreamError(store: ViewStore, stream: StreamId | null): HostError | null {
  const read = useCallback(
    () => (stream === null ? null : (store.error(stream) ?? null)),
    [store, stream],
  );
  return useSyncExternalStore(store.subscribe, read, read);
}
