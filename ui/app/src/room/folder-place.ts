// Where the open matter's folder lives, as list_papers says (here, on a network share, or synced by
// a cloud service): for the thread's change note, whose lines after an undo name the folder's sync
// service. Small and alone, so the thread's chunk carries no more of the room than this.
import { createContext } from "react";
import type { FolderPlace } from "../transport/protocol.ts";

/** The folder's place; undefined until the papers are listed, or outside a room. */
export const FolderPlaceContext = createContext<FolderPlace | undefined>(undefined);
