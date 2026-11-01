// Scripted NextHost answers (host-next.ts) for tests, the probe build and the copy review: the matters with their
// details, H8's papers, H7's settings and H20's default location. Not shipped: nothing room.tsx imports reaches it.
import type { FakeNextHost } from "../transport/fake-transport.ts";
import type { MatterSummary } from "../transport/protocol.ts";
import { DEFAULT_UI } from "./display-store.ts";
import type { MatterInfo, PaperSummary } from "./host-next.ts";

/** A matter with details: unpinned, its folder found, idle, unless `over` says otherwise. */
export const detailed = (m: MatterSummary, over: Partial<MatterInfo> = {}): MatterInfo => ({
  ...m,
  pinned: false,
  folder_state: "found",
  status: "idle",
  ...over,
});

/** A NextHost's answers: `matters` read on each list_matters, papers for every matter (none without), the default
 * location (none: the read fails), and the commands' answers. */
export function fakeNext({
  matters,
  papers = [],
  location = null,
  command,
}: {
  matters: () => readonly MatterInfo[];
  papers?: readonly PaperSummary[];
  location?: string | null;
  command?: FakeNextHost["command"];
}): FakeNextHost {
  return {
    query: (query) => {
      if (query === "list_matters") return { matters: [...matters()] };
      if (query === "get_settings") return { settings: { ui: DEFAULT_UI, locked: [] } };
      if (query === "get_default_location") {
        if (location === null) throw new Error("no default location in this host");
        return { default_location: { display: location } };
      }
      if ("list_papers" in query)
        return { papers: { papers: [...papers], skipped: [], truncated: false } };
      return { threads: [] };
    },
    ...(command === undefined ? {} : { command }),
  };
}
