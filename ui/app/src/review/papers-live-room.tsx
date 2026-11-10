// The room for the wall-following-the-folder entries (registry-papers-live.tsx): the one matter's folder, and what it
// does a second after the room shows, as the host's watch reports it (decision record 0055).
import { useEffect, useState } from "react";
import type { MatterSummary, PaperSummary } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "../room/fake-next.ts";
import { Room } from "../room/room.tsx";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";

const MATTER: MatterSummary = {
  matter: uuid(7, 2),
  name: "Sharma v. State of Delhi",
  kind: "folder",
};

const paper = (n: number, path: string): PaperSummary => ({
  file: uuid(n, 8),
  path,
  size: 1000 + n * 37,
  modified: 1_730_000_000_000,
});

/** What the folder does a moment after the room shows: nothing, a paper added, or the folder deleted. */
export type Change = "none" | "added" | "gone";

/** The host of the one matter whose folder holds `folder.papers`, or is gone. */
function folderHost() {
  const folder = { papers: [paper(1, "Plaint.pdf")], gone: false };
  const row = { info: detailed(MATTER, { folder_display: "Documents/Sharma" }) };
  const transport = new FakeTransport({
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      if (typeof query === "object" && query.list_threads !== undefined) return { threads: [] };
      throw new Error(`the review's host doesn't answer ${JSON.stringify(query)}`);
    },
    next: fakeNext({
      matters: () => [row.info],
      papers: () => {
        if (folder.gone)
          throw {
            code: "matter.folder_missing",
            message: "the review's gone folder",
            retryable: true,
          };
        return folder.papers;
      },
    }),
  });
  return { transport, folder, row };
}

/** The room, and a second after it shows, the folder's `change` as the host's watch reports it. */
export function LiveRoom({ change }: { change: Change }) {
  const [host] = useState(folderHost);
  const [store] = useState(() => createViewStore(host.transport, { batch: () => frameBatch }));
  useEffect(() => {
    if (change === "none") return;
    const timer = setTimeout(() => {
      const { transport, folder, row } = host;
      if (change === "added") folder.papers = [...folder.papers, paper(2, "Reply to notice.docx")];
      else {
        folder.gone = true;
        row.info = { ...row.info, folder_state: "missing" };
        transport.appEvent({ matters_changed: { matters: [row.info], removed: [], all: false } });
      }
      transport.appEvent({ papers_changed: { matter: MATTER.matter } });
    }, 1000);
    return () => clearTimeout(timer);
  }, [host, change]);
  return (
    <StoreContext value={store}>
      <Room transport={host.transport} />
    </StoreContext>
  );
}
