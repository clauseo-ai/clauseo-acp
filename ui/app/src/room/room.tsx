// The room's frame, as the approved room composes it (design/03-system/src/pages/room/room.tsx): one RoomScopeProvider
// with Base UI's CSP mode inside, then the window: the toolbar, the rail, the work and the wall, laid out by shell.css
// and responsive.css. Three columns from 1200 px, the rail folded behind Matters as a floating panel from 1000 px, one
// column below. The rail is U2's (rail.tsx over matters.ts); the work is the live thread (thread/work.tsx). The window
// draws its own frame
// (H19): the toolbar is the title bar, with the caption buttons wired to the window off the Mac and macOS's own traffic
// lights over the corner lights' place on it, inside the desktop app; elsewhere (tests, the probe, a browser) the
// buttons log their presses and the lights are painted. Until U12 there is no plan, until a host query no search, and
// until H5 nothing under a matter's title (the scratch matter's line stands). The wall
// shows its surface only until H8 (host-next.ts's papers part). Folded, the rail closes on Escape and on opening a
// matter or making one, and focus goes back to Matters; Matters carries the court-yellow dot while another matter
// waits on the lawyer.
import { use, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { EmptyPapers } from "@clauseo/components/clauseo/empty.tsx";
import { PaperTile } from "@clauseo/components/clauseo/paper-tile.tsx";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { RoomToolbar } from "@clauseo/components/clauseo/room-toolbar.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
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
import { LocateGate, MovedFolder } from "./moved.tsx";
import { RoomRail } from "./rail.tsx";
import { ScratchLine } from "./scratch-line.tsx";
import type { StandIns } from "./versions/stand-ins.ts";
import { useVersions } from "./versions/use-versions.ts";
import type { Drafts } from "./thread/session.ts";
import { Work } from "./thread/work.tsx";
import { tauriWindowControls, type WindowControls } from "../transport/window-controls.ts";
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
export function Room({
  transport,
  windowControls,
  standIns,
}: {
  transport: HostTransport;
  /** Optional, for tests: the window's controls in place of Tauri's, which the room makes itself inside the desktop
   * app. */
  windowControls?: WindowControls | null | undefined;
  /** Optional, for tests and the review page: what Versions can do once host changes land (versions/stand-ins.ts). */
  standIns?: StandIns | undefined;
}) {
  return (
    <RoomScopeProvider>
      <PartsRoot>
        <RoomFrame transport={transport} windowControls={windowControls} standIns={standIns} />
      </PartsRoot>
    </RoomScopeProvider>
  );
}

function RoomFrame({
  transport,
  windowControls,
  standIns,
}: {
  transport: HostTransport;
  windowControls: WindowControls | null | undefined;
  standIns: StandIns | undefined;
}) {
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
  // the app's view store; a room mounted alone (tests, the review) makes its own, and gives it to the work
  const shared = use(StoreContext);
  const [store] = useState(() => shared ?? createViewStore(transport, { batch: () => frameBatch }));
  const [matters] = useState(() => new Matters(transport, store));
  // each matter's composer draft, kept here, above Work's key, so it outlives a visit to another matter
  const [drafts] = useState<Drafts>(() => new Map());
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
  const { platform } = useDisplay();
  // the window's own controls, inside the desktop app only
  const [controls] = useState(() =>
    windowControls !== undefined
      ? windowControls
      : transport.kind === "tauri"
        ? tauriWindowControls()
        : null,
  );
  // the caption buttons' commands, off the Mac: its own lights need none, and capabilities/frame.json grants none
  // there, so asking whether the window is maximised would only be refused, on every resize
  const wired = useMemo(() => (platform === "mac" ? null : controls), [platform, controls]);
  const caption = useWindowFrame(wired);
  // Versions, for the open matter: the timeline in the rail's place while open, the band over the work
  const versions = useVersions({
    transport,
    store,
    matter: open?.matter ?? null,
    papers,
    platform,
    ...(standIns === undefined ? {} : { standIns }),
  });
  const title =
    open !== null
      ? scratch
        ? w.t("Scratch matter")
        : open.name
      : view.phase === "ready"
        ? frame(w.lang, "No matters yet")
        : "";
  return (
    <div
      className={versions.open ? "room-window ver-window" : "room-window"}
      data-panel={versions.open ? "" : undefined}
      ref={nodes.slot("window")}
    >
      <RoomToolbar
        chrome="own"
        caption={caption}
        nativeLights={controls !== null}
        search={false}
        versions={open !== null}
        {...versions.toolbar}
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
        railOpen={railOpen}
        onRail={() => {
          // the rail's place holds Versions' timeline: Matters closes it, and the rail comes back
          versions.close();
          setRailOpen((open) => !open);
        }}
        railWaits={railWaits(view)}
        railRef={railToggle}
        testIds={{ title: "matter", rail: "rail-toggle" }}
      />
      {/* Matters opens and folds it below 1200 px; opening a matter, making one or Escape folds it again */}
      {versions.rail ?? (
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
      )}
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
      <main className="room-centre" ref={nodes.slot("centre")}>
        {open !== null && view.folderMissing ? (
          // the folder moved (H3): its line in the thread's place, and Locate folder as the composer's gate
          <>
            <MovedFolder
              {...(open.folder_display === undefined ? {} : { folder: open.folder_display })}
              onLocate={() => void matters.locate(open.matter)}
            />
            <LocateGate onLocate={() => void matters.locate(open.matter)} />
          </>
        ) : (
          <StoreContext value={store}>
            {versions.centre}
            {scratch && <ScratchLine />}
            {/* one thread's state per matter: a new matter starts its own; with none open, the composer waits */}
            <Work
              key={open?.matter ?? "none"}
              transport={transport}
              matter={open}
              drafts={drafts}
              onThread={matters.onThread}
            />
          </StoreContext>
        )}
      </main>
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
