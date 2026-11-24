// How another part of the room opens Skills and connectors: the room gives OpenLibraryContext
// (room.tsx, from library/room-place.tsx), and a part inside it asks for the opener. The home
// card's "More about it" (room/connectors/case-law-offer.tsx) opens Connectors with Clauseo Case
// Law picked:
//
//   const openCaseLaw = useOpenCaseLaw();   null outside the room, where there is no place
//   openCaseLaw?.();
import { createContext, use } from "react";
import { CASE_LAW } from "../store/connectors.ts";

/** Where the place opens: a part, and an item picked. */
export type LibraryStart = { part?: "skills" | "connectors"; selected?: string | null };

/** Opens the place at `start` (Skills, nothing picked, when left out). */
export type OpenLibrary = (start?: LibraryStart) => void;

export const OpenLibraryContext = createContext<OpenLibrary | null>(null);

/** Connectors, with Clauseo Case Law's card picked. */
export const CASE_LAW_START: LibraryStart = { part: "connectors", selected: CASE_LAW };

/** The room's opener; null outside the room. */
export const useOpenLibrary = (): OpenLibrary | null => use(OpenLibraryContext);

/** Opens the place on Clauseo Case Law; null outside the room. */
export function useOpenCaseLaw(): (() => void) | null {
  const open = use(OpenLibraryContext);
  return open === null ? null : () => open(CASE_LAW_START);
}
