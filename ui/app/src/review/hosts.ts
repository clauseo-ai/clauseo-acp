// Scripted hosts for the review's room states: a FakeTransport that answers list_matters, and, where a state needs the
// wall, H8's list_papers (room/host-next.ts), as room.test.tsx and the probe script them.
import type { MatterSummary } from "../transport/protocol.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import type { PaperSummary } from "../room/host-next.ts";
import { DEFAULT_UI } from "../room/display-store.ts";

/** A matter as filed, for states that show one. */
export const MATTER: MatterSummary = {
  matter: "00000000-0000-4000-8000-000000000001",
  name: "Acme Traders v. State",
  kind: "folder",
};

/** A host with these matters; with papers, also H8's list of the matter's papers. */
export function roomHost(matters: MatterSummary[], papers?: PaperSummary[]): FakeTransport {
  const host: FakeHost = {
    query: (query) => {
      if (query === "list_matters") return { matters };
      throw new Error(`the review's host answers list_matters only, not ${JSON.stringify(query)}`);
    },
  };
  if (papers !== undefined) {
    host.next = {
      query: (query) =>
        query === "get_settings"
          ? { settings: { ui: DEFAULT_UI, locked: [] } }
          : { papers: { papers, skipped: [], truncated: false } },
    };
  }
  return new FakeTransport(host);
}
