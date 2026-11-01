// The room's frame, as the approved room composes it (design/03-system/src/pages/room/room.tsx): one RoomScopeProvider
// with Base UI's CSP mode inside, then the window: the toolbar, the rail, the work and the wall, laid out by shell.css
// and responsive.css. Three columns from 1200 px, the rail folded behind Matters as a floating panel from 1000 px, one
// column below. The rail and the work are slots for U2 and U3. Until H19 the computer draws the title bar, so the
// toolbar draws no window controls; until U12 there is no plan, until a host query no search, until U8 no Versions and
// Undo, and until H5 nothing under the title. The wall shows its surface only until H8 (host-next.ts).
import { useEffect, useState } from "react";
import { EmptyPapers } from "@clauseo/components/clauseo/empty.tsx";
import { PaperTile } from "@clauseo/components/clauseo/paper-tile.tsx";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { RoomToolbar } from "@clauseo/components/clauseo/room-toolbar.tsx";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { pickReopenMatter } from "../store/reopen.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterId, MatterSummary } from "../transport/protocol.ts";
import { nextHostOf, type NextHost, type Papers } from "./host-next.ts";

/** The matter the room opens on: the newest (store/reopen.ts) until U2's rail picks one. */
type Opened =
  | { state: "loading" }
  | { state: "open"; matter: MatterSummary }
  | { state: "none" }
  | { state: "failed" };

function useOpenedMatter(transport: HostTransport): Opened {
  const [opened, setOpened] = useState<Opened>({ state: "loading" });
  useEffect(() => {
    let live = true;
    transport.query("list_matters").then(
      (result) => {
        if (!live) return;
        const matter = pickReopenMatter(result.matters ?? []);
        setOpened(matter === null ? { state: "none" } : { state: "open", matter });
      },
      (error: unknown) => {
        if (!live) return;
        console.warn(`list_matters failed: ${JSON.stringify(error)}`);
        setOpened({ state: "failed" });
      },
    );
    return () => {
      live = false;
    };
  }, [transport]);
  return opened;
}

/** The matter's papers from H8, or null: before they arrive, without a NextHost, or when the read fails. */
function usePapers(next: NextHost | null, matter: MatterId | null): Papers | null {
  const [listed, setListed] = useState<{ matter: MatterId; papers: Papers } | null>(null);
  useEffect(() => {
    if (next === null || matter === null) return;
    let live = true;
    next.listPapers(matter).then(
      (papers) => {
        if (live) setListed({ matter, papers });
      },
      (error: unknown) => console.warn(`list_papers failed: ${JSON.stringify(error)}`),
    );
    return () => {
      live = false;
    };
  }, [next, matter]);
  return listed !== null && listed.matter === matter ? listed.papers : null;
}

/** A file's name, the last part of its path in the matter folder. */
const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** The room: its own scope, and Base UI's CSP mode for everything inside it, so no part writes a style element. */
export function Room({ transport }: { transport: HostTransport }) {
  return (
    <RoomScopeProvider>
      <PartsRoot>
        <RoomFrame transport={transport} />
      </PartsRoot>
    </RoomScopeProvider>
  );
}

function RoomFrame({ transport }: { transport: HostTransport }) {
  const w = useWords();
  const room = useRoom();
  const { nodes } = room;
  const [railOpen, setRailOpen] = useState(false);
  const [next] = useState(() => nextHostOf(transport));
  const opened = useOpenedMatter(transport);
  const papers = usePapers(next, opened.state === "open" ? opened.matter.matter : null);
  // a matter's name as filed is English in both languages; with no matter, the frame's words stand in its place
  const title =
    opened.state === "open"
      ? opened.matter.name
      : opened.state === "none"
        ? frame(w.lang, "No matters yet")
        : "";
  return (
    <div className="room-window" ref={nodes.slot("window")}>
      <RoomToolbar
        chrome="os"
        search={false}
        versions={false}
        plan={null}
        title={title}
        titleLang={opened.state === "open" ? "en" : w.lang}
        titleHint=""
        subtitle=""
        nextDate=""
        caseNo=""
        court=""
        history={[]}
        canUndo={false}
        onUndo={() => {}}
        railOpen={railOpen}
        onRail={() => setRailOpen((open) => !open)}
        testIds={{ title: "matter" }}
      />
      {/* U2's rail goes here; Matters opens and folds it below 1200 px */}
      <div className="room-rail" id={room.id("rail")} data-open={railOpen ? "" : undefined} />
      {/* U3's thread and composer go here */}
      <main className="room-centre" ref={nodes.slot("centre")} />
      <aside className="room-pane" ref={nodes.slot("pane")}>
        {papers !== null &&
          (papers.papers.length === 0 ? (
            <EmptyPapers body={false} />
          ) : (
            <ol className="facade facade-names">
              {papers.papers.map((paper) => (
                <PaperTile
                  key={paper.file}
                  id={paper.file}
                  state="uncited"
                  title={fileName(paper.path)}
                />
              ))}
            </ol>
          ))}
      </aside>
    </div>
  );
}
