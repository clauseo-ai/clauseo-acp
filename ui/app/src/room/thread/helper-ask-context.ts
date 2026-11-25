// The stream a row is in and an older page's entries, which row-of.tsx gives its rows so a helper's
// ask can name the helper (helper-asker.tsx). Apart from the component for fast refresh.
import { createContext } from "react";
import type { EntryId, StreamId, ViewEntry } from "../../transport/protocol.ts";

export const HelperAskContext = createContext<{
  stream: StreamId;
  paged: ReadonlyMap<EntryId, ViewEntry>;
} | null>(null);
