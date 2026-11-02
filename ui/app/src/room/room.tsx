// The room's frame, as the approved room composes it (design/03-system/src/pages/room/room.tsx): one RoomScopeProvider
// with Base UI's CSP mode inside, then the window: the toolbar, the rail, the work and the wall, laid out by shell.css
// and responsive.css. Three columns from 1200 px, the rail folded behind Matters as a floating panel from 1000 px, one
// column below. The rail is U2's (rail.tsx over matters.ts); the work is the live thread (thread/work.tsx). The window
// draws its own frame
// (H19): the toolbar is the title bar, with the caption buttons wired to the window off the Mac and macOS's own traffic
// lights over the corner lights' place on it, inside the desktop app; elsewhere (tests, the probe, a browser) the
// buttons log their presses and the lights are painted. Until U12 there is no plan, until a host query no search, and
// until H5 nothing under a matter's title (the scratch matter's line stands). The wall
// lists H8's papers (host-next.ts's papers part, which the desktop gives through host-wire.ts). Folded, the rail closes
// on Escape and on opening a matter or making one, and focus goes back to Matters; opening a conversation or starting
// one closes it too, and the field takes the focus. Matters carries the court-yellow dot while another matter waits on
// the lawyer. Cmd+N on the Mac, Ctrl+N elsewhere, starts a new conversation in the open matter; until its first
// question, the reading column's top says what it reads, once the matter's papers are counted. A link or paper in an
// answer opens through the host (open.ts, H11), and so does a matter row's Show in Finder; a Word document's tile, and a
// change note naming one, open it in Word (word/open-in-word.ts). Versions opens and shows files through the
// transport's own open_file and reveal unless a test or the review page passes stand-ins.
import {
  lazy,
  Suspense,
  use,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { EmptyPapers } from "@clauseo/components/clauseo/empty.tsx";
import { PaperTile } from "@clauseo/components/clauseo/paper-tile.tsx";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { RoomToolbar } from "@clauseo/components/clauseo/room-toolbar.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useWords } from "@clauseo/components/lib/words.ts";
import { MarkdownHostContext, type MarkdownHost } from "@clauseo/components/markdown/host.ts";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { frameBatch } from "../store/batch.ts";
import { StoreContext } from "../store/hooks.ts";
import { createViewStore } from "../store/store.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { Papers, PapersHost } from "./host-next.ts";
import { knownPlan } from "./agent.ts";
import { Conversations } from "./conversations.ts";
import { Matters, type MattersView } from "./matters.ts";
import { NewMatter } from "./new-matter.tsx";
import { AnswerHost, type ListedPapers } from "./open.ts";
import { LocateGate, MovedFolder } from "./moved.tsx";
import { RoomRail } from "./rail.tsx";
import { ScratchLine } from "./scratch-line.tsx";
import type { StandIns } from "./versions/stand-ins.ts";
import { useVersions } from "./versions/use-versions.ts";
import { useWordOpeners, WordOpenContext } from "./word/open-in-word.ts";
import { clockOf } from "./when.ts";
import type { Drafts } from "./thread/session.ts";
import { Work } from "./thread/work.tsx";
import { tauriWindowControls, type WindowControls } from "../transport/window-controls.ts";
import { useWindowFrame } from "../window/use-window-frame.ts";

/** H13's line atop a retried conversation, loaded only when one opens. */
const RetriedLine = lazy(() =>
  late(import("./retried-line.tsx")).then((m) => ({ default: m.RetriedLine })),
);

/** The matter's papers from H8, or null: before they arrive, without H8, or when the read fails. */
function usePapers(next: PapersHost | null, matter: MatterId | null): ListedPapers {
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
  return listed !== null && listed.matter === matter ? listed : null;
}

/** The answers' MarkdownHost (open.ts): one object per transport and matter, so a new papers list or a streaming row
 * never makes a new one; the wall's papers reach it as they arrive. */
function useAnswerHost(
  transport: HostTransport,
  matter: MatterId | null,
  listed: ListedPapers,
): MarkdownHost {
  const host = useMemo(() => new AnswerHost(transport, matter), [transport, matter]);
  useEffect(() => host.show(listed), [host, listed]);
  return host;
}

/** What Versions opens and shows files with in the desktop app: the transport's own (H11). */
function useTransportStandIns(transport: HostTransport): StandIns {
  return useMemo(
    () => ({
      openFile: (file) => transport.openFile(file),
      reveal: (target) => transport.reveal(target),
    }),
    [transport],
  );
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
  // each conversation's composer draft, kept here, above Work's key, so it outlives a visit to another conversation
  const [drafts] = useState<Drafts>(() => new Map());
  const [conversations] = useState(() => new Conversations(transport, store, matters, drafts));
  useEffect(() => matters.start(), [matters]);
  useEffect(() => conversations.start(), [conversations]);
  const view = useSyncExternalStore(matters.subscribe, matters.view, matters.view);
  const talks = useSyncExternalStore(
    conversations.subscribe,
    conversations.view,
    conversations.view,
  );
  const link = useMemo(
    () => ({
      thread: talks.thread,
      create: conversations.create,
      sent: conversations.sent,
      refused: talks.refused,
      ...(talks.canRetry ? { retry: conversations.retry } : {}),
    }),
    [talks.thread, talks.refused, talks.canRetry, conversations],
  );
  const retried = talks.retried;
  // a new conversation's plan, offered where the host lists more than one, until its first question fixes it
  const choice = useMemo(
    () =>
      talks.drafting && talks.options.length > 1
        ? {
            value: talks.agent,
            options: talks.options,
            segmented: talks.options,
            onValueChange: (agent: string) => conversations.choose(agent),
          }
        : undefined,
    [talks.drafting, talks.options, talks.agent, conversations],
  );
  const open = view.open;
  const listed = usePapers(matters.next.papers, open?.matter ?? null);
  const papers = listed?.papers ?? null;
  const answers = useAnswerHost(transport, open?.matter ?? null, listed);
  const word = useWordOpeners(transport, listed, w.lang);
  const ownStandIns = useTransportStandIns(transport);
  const { platform } = useDisplay();
  /** The folded rail closes, and focus goes back to Matters, which opened it. */
  const fold = () => {
    if (!railOpen) return;
    setRailOpen(false);
    railToggle.current?.focus();
  };
  // New conversation's key, while a matter's conversations are in and no dialog holds the keys; an input method's
  // composing key is its own
  const mac = platform === "mac";
  const canStart =
    open !== null &&
    !view.folderMissing &&
    !newOpen &&
    talks.matter === open.matter &&
    talks.phase !== "loading";
  useEffect(() => {
    if (!canStart) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing || e.keyCode === 229 || e.altKey || e.shiftKey) return;
      // the letter the layout gives (Dvorak's N is N, its Ctrl+B is not); the key's place only where the layout gives no
      // Latin letter (Devanagari, Cyrillic)
      const key = e.key.toLowerCase();
      if (/^[a-z]$/.test(key) ? key !== "n" : e.code !== "KeyN") return;
      if (mac ? !e.metaKey || e.ctrlKey : !e.ctrlKey || e.metaKey) return;
      e.preventDefault();
      conversations.startNew();
      setRailOpen(false);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [canStart, mac, conversations]);
  // the plan the open conversation uses, on the status line: its thread's, or the one a draft would run
  // nothing while the matter's list loads (its agent isn't known yet), or for an agent of no kind
  const openRow = talks.rows.find((r) => r.key === talks.open);
  const planOfOpen =
    open === null || talks.open === null || talks.phase === "loading"
      ? null
      : knownPlan(openRow?.agent ?? talks.agent, talks.agents);
  const plan = planOfOpen === null ? "" : th(w.lang, "Uses your {name} plan", { name: planOfOpen });
  // a matter's name as filed is English in both languages, the scratch matter's is the room's own; with no matter, the
  // frame's words stand in its place
  const scratch = open?.kind === "scratch";
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
    standIns: standIns ?? ownStandIns,
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
      className="room-window"
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
          // the rail's place holds Versions' timeline: Matters closes it, and the rail comes back; with the panel put
          // away on a mid window the rail stands already, and Versions stays on the version in view
          if (versions.open) versions.close();
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
          conversations={conversations}
          talks={talks}
          open={railOpen}
          onNewMatter={() => {
            madeFolded.current = false;
            setNewOpen(true);
          }}
          onMoved={fold}
          onConversation={() => setRailOpen(false)}
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
            {retried !== null && (
              <Suspense fallback={null}>
                <RetriedLine
                  time={clockOf(retried.at, view.now, view.zone, w.lang).time}
                  title={retried.title ?? mt(w.lang, "New conversation")}
                  onOpen={() => conversations.open(retried.from)}
                />
              </Suspense>
            )}
            {/* a new conversation, before its first question: what it reads, once the papers are counted. A matter with
                no conversation yet shows the same line: the design's FirstQuestion (empty.tsx: "Clauseo has read this
                matter's {n} papers.", and that each answer cites its page) goes here once stage D reads the papers
                ahead and answers carry citations; until then those are promises the room can't keep */}
            {talks.drafting && !scratch && papers !== null && papers.papers.length > 0 && (
              <p
                className="mx-auto box-content max-w-measure px-thread-pad pt-s7 pb-s4 text-center text-reply text-primary"
                data-testid="new-conversation-line"
              >
                <Say>
                  {papers.papers.length === 1
                    ? mt(w.lang, "A new conversation. It reads this matter's one paper.")
                    : mt(w.lang, "A new conversation. It reads this matter's {n} papers.", {
                        n: papers.papers.length,
                      })}
                </Say>
              </p>
            )}
            {/* one thread's state per conversation: each starts its own; with no matter open, the composer waits */}
            <MarkdownHostContext value={answers}>
              <WordOpenContext value={word}>
                <Work
                  key={talks.open ?? open?.matter ?? "none"}
                  transport={transport}
                  matter={open}
                  drafts={drafts}
                  draftKey={talks.open}
                  conversation={link}
                  choice={choice}
                  plan={plan}
                  focus={talks.focus?.n ?? null}
                  onThread={matters.onThread}
                />
              </WordOpenContext>
            </MarkdownHostContext>
          </StoreContext>
        )}
      </main>
      <aside className="room-pane" ref={nodes.slot("pane")}>
        {papers !== null &&
          (papers.papers.length === 0 ? (
            <EmptyPapers body={false} />
          ) : (
            <ol className="facade facade-names">
              {papers.papers.map((paper) => {
                const opener = word.openerFor(paper.path);
                return (
                  <PaperTile
                    key={paper.file}
                    id={paper.file}
                    state="uncited"
                    title={fileName(paper.path)}
                    {...(opener === null
                      ? {}
                      : {
                          onTurn: opener.onOpen,
                          tip: opener.label,
                          label: opener.label,
                          testIds: { face: "open-in-word" },
                        })}
                  />
                );
              })}
            </ol>
          ))}
      </aside>
    </div>
  );
}
