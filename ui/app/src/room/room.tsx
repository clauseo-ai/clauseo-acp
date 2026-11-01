// The room's frame, as the approved room composes it (design/03-system/src/pages/room/room.tsx): one RoomScopeProvider
// with Base UI's CSP mode inside, then the window: the toolbar, the rail, the work and the wall, laid out by shell.css
// and responsive.css. Three columns from 1200 px, the rail folded behind Matters as a floating panel from 1000 px, one
// column below. The rail is U2's (rail.tsx over matters.ts); the work is U3's slot. The window draws its own frame
// (H19): the toolbar is the title bar, with the caption buttons wired to the window off the Mac and macOS's own traffic
// lights over the corner lights' place on it, inside the desktop app; elsewhere (tests, the probe, a browser) the
// buttons log their presses and the lights are painted. Until U12 there is no plan, until a host query no search, until
// U8 no Versions and Undo, and until H5 nothing under a matter's title (the scratch matter's line stands). The wall
// shows its surface only until H8 (host-next.ts's papers part). Folded, the rail closes on Escape and on opening a
// matter or making one, and focus goes back to Matters; Matters carries the court-yellow dot while another matter
// waits on the lawyer.
import { use, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { EmptyPapers } from "@clauseo/components/clauseo/empty.tsx";
import { PaperTile } from "@clauseo/components/clauseo/paper-tile.tsx";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { RoomToolbar } from "@clauseo/components/clauseo/room-toolbar.tsx";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { Papers, PapersHost } from "./host-next.ts";
import { Matters, type MattersView } from "./matters.ts";
import { NewMatter } from "./new-matter.tsx";
import { RoomRail } from "./rail.tsx";
import { tauriWindowControls } from "../transport/window-controls.ts";
import { useWindowFrame } from "../window/use-window-frame.ts";

/** The matter's papers from H8, or null: before they arrive, without H8, or when the read fails. */
function usePapers(next: PapersHost | null, matter: MatterId | null): Papers | null {
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

/** Whether a matter the lawyer isn't looking at waits on them, with a question or for permission: Matters' dot. The
 * limit stays muted (QU.6 (ii)): nothing to choose, only a time to wait for. */
const railWaits = (view: MattersView): boolean =>
  [...view.status].some(
    ([id, s]) => id !== view.open?.matter && (s.state === "question" || s.state === "permission"),
  );

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
  const [newOpen, setNewOpen] = useState(false);
  const newMatter = useRef<HTMLButtonElement>(null);
  const railToggle = useRef<HTMLButtonElement>(null);
  // a matter made from the folded rail folds it, so focus goes back to Matters, not to New matter's hidden button
  const madeFolded = useRef(false);
  const [newFocus] = useState(() => ({
    get current(): HTMLElement | null {
      return madeFolded.current ? railToggle.current : newMatter.current;
    },
  }));
  // the app's view store; a room mounted alone (tests, the review) makes its own
  const shared = use(StoreContext);
  const [matters] = useState(
    () => new Matters(transport, shared ?? createViewStore(transport, { batch: () => frameBatch })),
  );
  useEffect(() => matters.start(), [matters]);
  const view = useSyncExternalStore(matters.subscribe, matters.view, matters.view);
  const open = view.open;
  const papers = usePapers(matters.next.papers, open?.matter ?? null);
  /** The folded rail closes, and focus goes back to Matters, which opened it. */
  const fold = () => {
    if (!railOpen) return;
    setRailOpen(false);
    railToggle.current?.focus();
  };
  // a matter's name as filed is English in both languages, the scratch matter's is the room's own; with no matter, the
  // frame's words stand in its place
  const scratch = open?.kind === "scratch";
  // the window's own controls, inside the desktop app only
  const [controls] = useState(() => (transport.kind === "tauri" ? tauriWindowControls() : null));
  const caption = useWindowFrame(controls);
  const title =
    open !== null
      ? scratch
        ? w.t("Scratch matter")
        : open.name
      : view.phase === "ready"
        ? frame(w.lang, "No matters yet")
        : "";
  return (
    <div className="room-window" ref={nodes.slot("window")}>
      <RoomToolbar
        chrome="own"
        caption={caption}
        nativeLights={controls !== null}
        search={false}
        versions={false}
        plan={null}
        title={title}
        titleLang={open !== null && !scratch ? "en" : w.lang}
        titleHint=""
        subtitle={scratch ? mt(w.lang, "Quick questions, kept like any matter") : ""}
        nextDate=""
        caseNo=""
        court=""
        history={[]}
        canUndo={false}
        onUndo={() => {}}
        railOpen={railOpen}
        onRail={() => setRailOpen((open) => !open)}
        railWaits={railWaits(view)}
        railRef={railToggle}
        testIds={{ title: "matter", rail: "rail-toggle" }}
      />
      {/* Matters opens and folds it below 1200 px; opening a matter, making one or Escape folds it again */}
      <RoomRail
        matters={matters}
        view={view}
        open={railOpen}
        onNewMatter={() => {
          madeFolded.current = false;
          setNewOpen(true);
        }}
        onMoved={fold}
        onEscape={fold}
        newMatterRef={newMatter}
      />
      <NewMatter
        matters={matters}
        detailed={view.detailed}
        open={newOpen}
        onOpenChange={setNewOpen}
        onMade={() => {
          madeFolded.current = railOpen;
          setRailOpen(false);
        }}
        finalFocus={newFocus}
      />
      {/* U3's work goes here, one per matter: {open !== null && <Work key={open.matter} transport={transport}
          matter={open} onThread={matters.onThread} />}; while view.folderMissing it shows moved.tsx's MovedFolder in the
          thread's place and LocateGate as the composer's gate, both with onLocate={() => matters.locate(open.matter)} */}
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
