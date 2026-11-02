// Versions' way into the room, the only part of it in the room's first load: whether Versions stands open for the open
// matter, and its chunk (versions-chunk.ts), loaded the first time the lawyer presses Versions or Undo. While open, the
// timeline stands where the rail stood (the room renders `rail` in the rail's place) and the band over the work
// (`centre`); closing it, or opening another matter, brings the rail back.
import { createElement, useEffect, useState, type ReactNode } from "react";
import type { Platform } from "@clauseo/components/lib/display.ts";
import { late } from "@clauseo/components/lib/late.ts";
import type { ViewStore } from "../../store/store.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { MatterId } from "../../transport/protocol.ts";
import type { Papers } from "../host-next.ts";
import type { VersionsController } from "./controller.ts";
import { standInsOf, type StandIns } from "./stand-ins.ts";

type Chunk = typeof import("./versions-chunk.ts");

let chunk: Promise<Chunk> | null = null;

/** Loads Versions' chunk and its sheet, once. */
export function loadVersions(): Promise<Chunk> {
  chunk ??= late(
    import("./versions-chunk.ts").then(
      async (m) => {
        await m.loadSheet();
        return m;
      },
      (error: unknown) => {
        // Versions stays shut, and the next press tries again
        chunk = null;
        throw error;
      },
    ),
  );
  return chunk;
}

export type VersionsSlots = {
  /** Whether the timeline stands in the rail's place. */
  open: boolean;
  /** The timeline, for the rail's place, or null: the rail stands. */
  rail: ReactNode;
  /** The band over the work, with the restore dialog, or null. */
  centre: ReactNode;
  /** For the toolbar's Versions and Undo pair. */
  toolbar: { onVersions: () => void; versionsOpen: boolean; onUndo: () => void };
};

export function useVersions({
  transport,
  store,
  matter,
  papers,
  platform,
  standIns: given,
}: {
  transport: HostTransport;
  store: ViewStore;
  matter: MatterId | null;
  papers: Papers | null;
  platform: Platform;
  /** Optional, for tests and the review page: the host changes still to land. The transport's own by default. */
  standIns?: StandIns;
}): VersionsSlots {
  const [own] = useState(() => standInsOf(transport));
  const standIns = given ?? own;
  // the open Versions: its chunk, and its controller for the matter it opened on
  const [session, setSession] = useState<{
    matter: MatterId;
    mod: Chunk;
    controller: VersionsController;
  } | null>(null);
  // Versions belongs to the matter it opened on: opening another closes it
  const live = session !== null && session.matter === matter ? session : null;
  useEffect(() => live?.controller.start(), [live]);

  /** Opens Versions on the open matter, once its chunk is in; undo: and asks to undo the newest change. */
  const show = (undo: boolean) => {
    if (matter === null) return;
    if (live !== null) {
      if (undo) live.controller.undoNewest();
      return;
    }
    loadVersions().then(
      (mod) => {
        const controller = new mod.VersionsController(transport, store, matter);
        if (undo) controller.undoNewest();
        setSession({ matter, mod, controller });
      },
      (error: unknown) => console.warn(`Versions' chunk did not load: ${String(error)}`),
    );
  };
  const close = () => setSession(null);
  const fileRef = (path: string) => papers?.papers.find((p) => p.path === path)?.file ?? null;
  if (live === null)
    return {
      open: false,
      rail: null,
      centre: null,
      toolbar: { onVersions: () => show(false), versionsOpen: false, onUndo: () => show(true) },
    };
  const { mod, controller } = live;
  return {
    open: true,
    rail: createElement(mod.VersionsRail, { controller, platform, standIns, onClose: close }),
    centre: createElement(mod.VersionsCentre, { controller, standIns, fileRef }),
    toolbar: { onVersions: close, versionsOpen: true, onUndo: () => show(true) },
  };
}
