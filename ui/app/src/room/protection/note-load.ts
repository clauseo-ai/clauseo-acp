// UC: the protection parts outside the room's first load (note-chunk.ts: each cause's note, the "Unknown publisher"
// confirm, the waiting line, the firm's notice, the first run's step parts and words-protection.ts), loaded once a Not
// protected matter opens, a press needs them, or the first run runs over the protection wire; and LazyPart, how the
// room places them. The chunk keeps Vite's preload list (vite.config.ts's PRELOADED_CHUNKS): it takes the alert
// dialog's primitives, which the first load lacks. Written with createElement, so this file holds no component file's
// JSX and may export the loader beside it.
import { late } from "@clauseo/components/lib/late.ts";
import { createElement, lazy, Suspense, type ComponentProps } from "react";

type NoteModule = typeof import("./note-chunk.ts");

let loading: Promise<NoteModule> | null = null;

/** Starts loading the chunk, once; later calls give back the same import. */
export const loadProtectionNote = (): Promise<NoteModule> =>
  (loading ??= late(import("./note-chunk.ts")));

const Part = lazy(() => loadProtectionNote().then((m) => ({ default: m.ProtectionPart })));

/** The note or the firm's notice, once the chunk is in; nothing before. */
export const LazyPart = (props: ComponentProps<NoteModule["ProtectionPart"]>) =>
  createElement(Suspense, { fallback: null }, createElement(Part, props));
