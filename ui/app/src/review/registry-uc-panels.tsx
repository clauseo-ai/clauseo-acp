// The toolbar's panel toggles' copy review entries (UC-252 to UC-255): the sidebar's toggle and the papers' toggle,
// each named and tipped by what a press does, in the real room over FakeTransport at a wide Mac window (1512 by 945),
// where the rail has its column. "Show" is the state after the toggle is pressed once; the press keeps the choice for
// the window (layout-store.ts), and every cell shares the page's storage, so the choice is let go in the same task,
// before another cell can read it. Hindi provisional.
import type { ToolbarKey } from "@clauseo/components/lib/words-toolbar.ts";
import type { Entry } from "./entry.ts";
import { MATTER, roomHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { RoomPanelHidden } from "./states-panels.tsx";
import { toolbarWords } from "./words.ts";

/** A wide window, where the rail stands beside the work. */
const WIDE = { width: 1512, height: 945 } as const;
const host = () => roomHost([MATTER]);

const entry = (id: string, key: ToolbarKey, press?: string): Entry => ({
  id,
  words: toolbarWords(key),
  screen: "room/toolbar",
  ...WIDE,
  render: () =>
    press === undefined ? <RoomOver host={host} /> : <RoomPanelHidden host={host} press={press} />,
});

export const UC_PANELS: Entry[] = [
  entry("UC-252", "Hide the sidebar"),
  entry("UC-253", "Show the sidebar", "rail-toggle"),
  entry("UC-254", "Hide the papers"),
  entry("UC-255", "Show the papers", "wall-toggle"),
];
