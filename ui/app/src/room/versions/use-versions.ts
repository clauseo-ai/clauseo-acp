// Versions' way into the room, the only part of it in the room's first load: whether Versions stands open for the open
// matter, and its chunk (versions-chunk.ts), loaded the first time the lawyer presses Versions or Undo. While open, the
// timeline stands where the rail stood (the room renders `rail` in the rail's place) and the band over the work
// (`centre`); closing it, or opening another matter, brings the rail back.
import {
  createElement,
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import type { Platform } from "@clauseo/components/lib/display.ts";
import { late } from "@clauseo/components/lib/late.ts";
import type { ViewStore } from "../../store/store.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { MatterId } from "../../transport/protocol.ts";
import type { Papers } from "../host-next.ts";
import type { VersionsController } from "./controller.ts";
import { NO_STAND_INS, type StandIns } from "./stand-ins.ts";

type Chunk = typeof import("./versions-chunk.ts");

let chunk: Promise<Chunk> | null = null;

/** Loads Versions' chunk, once, and its sheet: a sheet that failed is asked for again on the next open. */
export function loadVersions(): Promise<Chunk> {
  chunk ??= import("./versions-chunk.ts").catch((error: unknown) => {
    // Versions stays shut, and the next press tries again
    chunk = null;
    throw error;
  });
  return late(
    chunk.then(async (m) => {
      await m.loadSheet();
      return m;
    }),
  );
}

/** A mid window, where the rail folds behind Matters and Versions' panel opens over the work (responsive.css). */
const MID = "(min-width: 1000px) and (max-width: 1199px)";
const midQuery = () => (typeof matchMedia === "function" ? matchMedia(MID) : null);
const onMid = (fn: () => void) => {
  const q = midQuery();
  q?.addEventListener("change", fn);
  return () => q?.removeEventListener("change", fn);
};
const isMid = () => midQuery()?.matches ?? false;

export type VersionsSlots = {
  /** Whether the timeline stands in the rail's place. */
  open: boolean;
  /** The timeline, for the rail's place, or null: the rail stands. */
  rail: ReactNode;
  /** The band over the work, with the restore dialog, or null. */
  centre: ReactNode;
  /** For the toolbar's Versions and Undo pair; writing: Undo waits, faint, as while Clauseo writes: Versions is open
   * and nothing in the matter can be undone. */
  toolbar: { onVersions: () => void; versionsOpen: boolean; onUndo: () => void; writing: boolean };
  /** Closes Versions: the rail comes back. */
  close: () => void;
};

export function useVersions({
  transport,
  store,
  matter,
  papers,
  platform,
  standIns = NO_STAND_INS,
}: {
  transport: HostTransport;
  store: ViewStore;
  matter: MatterId | null;
  papers: Papers | null;
  platform: Platform;
  /** Optional, for tests and the review page: the host changes still to land. None by default. */
  standIns?: StandIns;
}): VersionsSlots {
  // the open Versions: its chunk, and its controller for the matter it opened on
  const [session, setSession] = useState<{
    matter: MatterId;
    mod: Chunk;
    controller: VersionsController;
  } | null>(null);
  // Versions belongs to the matter it opened on: opening another closes it, and coming back finds it closed
  if (session !== null && session.matter !== matter) setSession(null);
  const live = session !== null && session.matter === matter ? session : null;
  useEffect(() => live?.controller.start(), [live]);
  const watch = useCallback(
    (fn: () => void) => live?.controller.subscribe(fn) ?? (() => undefined),
    [live],
  );
  // whether the room looks at a version, back or the newest picked: the band stands with its acts
  const looks = () => {
    const look = live?.controller.snapshot().look;
    return look !== undefined && (look.kind === "back" || look.id !== undefined);
  };
  const looking = useSyncExternalStore(watch, looks, looks);
  // on a mid window the panel stands over the band: picking a version puts it away, as a menu goes, and the band's
  // Versions brings it back; while the room looks at a version, the panel's Close and Escape put it away again and
  // keep the look, and a window narrowed to mid puts it away; a wider window shows it again
  const mid = useSyncExternalStore(onMid, isMid, () => false);
  const [away, setAway] = useState(false);
  // the panel was brought back over the work: it takes the focus as it opens
  const [reopened, setReopened] = useState(false);
  const putPanel = (gone: boolean) => {
    setAway(gone);
    setReopened(false);
  };
  const [wasMid, setWasMid] = useState(mid);
  if (wasMid !== mid) {
    setWasMid(mid);
    if (mid) putPanel(looking);
  }
  const panel = !(mid && away);
  // the open matter, for a chunk that loads after the lawyer has moved on
  const current = useRef(matter);
  useEffect(() => {
    current.current = matter;
  }, [matter]);

  /** Opens Versions on the open matter, once its chunk is in; undo: and asks to undo the newest change. */
  const show = (undo: boolean) => {
    if (matter === null) return;
    if (live !== null) {
      if (undo) live.controller.undoNewest();
      return;
    }
    loadVersions().then(
      (mod) => {
        if (current.current !== matter) return;
        const controller = new mod.VersionsController(transport, store, matter);
        // Undo opened Versions: Cancel in its dialog closes it again
        if (undo) controller.undoNewest(true);
        putPanel(false);
        setSession({ matter, mod, controller });
      },
      (error: unknown) => console.warn(`Versions' chunk did not load: ${String(error)}`),
    );
  };
  // one function for the room's life, so the room's New conversation key needn't listen again each render
  const close = useCallback(() => setSession(null), []);
  // whether the open Versions found nothing to undo: the toolbar's Undo waits
  const nothing = () => {
    const s = live?.controller.snapshot();
    return s !== undefined && s.ready && !s.canUndo;
  };
  const undoWaits = useSyncExternalStore(watch, nothing, nothing);
  const fileRef = (path: string) => papers?.papers.find((p) => p.path === path)?.file ?? null;
  if (live === null)
    return {
      open: false,
      rail: null,
      centre: null,
      toolbar: {
        onVersions: () => show(false),
        versionsOpen: false,
        onUndo: () => show(true),
        writing: false,
      },
      close,
    };
  const { mod, controller } = live;
  const onPicked = () => putPanel(isMid());
  const onPanel = () => {
    setAway(false);
    setReopened(true);
  };
  // over the band on a mid window: put away, with the look kept
  const putAway = mid && looking ? () => putPanel(true) : null;
  return {
    open: panel,
    rail: panel
      ? createElement(mod.VersionsRail, {
          controller,
          platform,
          standIns,
          onClose: putAway ?? close,
          onPicked,
          focusOnOpen: reopened,
        })
      : null,
    centre: createElement(mod.VersionsCentre, {
      controller,
      standIns,
      fileRef,
      onLeave: close,
      onPanel: panel ? null : onPanel,
      onAway: panel ? putAway : null,
    }),
    toolbar: {
      onVersions: panel ? close : onPanel,
      versionsOpen: panel,
      onUndo: () => show(true),
      writing: undoWaits,
    },
    close,
  };
}
