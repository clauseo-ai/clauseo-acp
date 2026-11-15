// The room's two panels, the sidebar (the rail) and the papers (the wall), each shown or hidden by one toggle on the
// toolbar at every width, and by the View menu's items on the Mac or Ctrl+B and Ctrl+Alt+B elsewhere (room-commands.ts).
// From 1200 px the sidebar's toggle hides or shows the rail's column, and the choice is kept for the window
// (layout-store.ts); below, where the rail is folded, the same toggle opens and closes the folded rail's panel over the
// work, the room's `railOpen`, and leaves the kept choice alone. The papers' toggle hides or shows the wall at every
// width: its column from 1000 px (between 1000 and 1199 px it keeps its column, as the rail folds alone there), the
// papers stacked under the conversation below. The reading column keeps its measure either way (shell.css's
// --gutter). The window carries data-rail and data-wall, "hidden" for a hidden panel, for responsive.css; a panel
// hidden while it held the focus gives the focus to its toggle. On the Mac the desktop's View menu says what each item
// would do, so the page tells it the two shown flags whenever they change: below 1200 px the sidebar's is the panel's.
import { useRailShown } from "@clauseo/components/clauseo/rail-shown.ts";
import type { RoomScope } from "@clauseo/components/clauseo/room-scope.ts";
import { useEffect, useRef, useState, type RefObject } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import { windowLabel } from "../transport/window-label.ts";
import { readLayout, writeLayout, type Layout } from "./layout-store.ts";

export type RoomLayout = {
  /** Whether the rail stands beside the work: 1200 px and up, and the sidebar shown. */
  railBeside: boolean;
  /** The sidebar's toggle: shown (aria-expanded) and its press. */
  rail: { shown: boolean; toggle: () => void };
  /** The papers' toggle: shown and its press, the wall's id, and the button for the focus. */
  papers: {
    shown: boolean;
    onToggle: () => void;
    controls: string;
    ref: RefObject<HTMLButtonElement | null>;
  };
  /** Opens the sidebar where Find needs it: the folded rail's panel, or a hidden rail's column. */
  openRail: () => void;
  /** The window's attributes for responsive.css. */
  attrs: { "data-rail"?: "hidden"; "data-wall"?: "hidden" };
};

/** The room's layout, over the room's own folded-rail state and Versions, which stands in the rail's place. */
export function useRoomLayout({
  transport,
  room,
  mac,
  railOpen,
  setRailOpen,
  railToggle,
  versions,
}: {
  transport: HostTransport;
  room: RoomScope;
  mac: boolean;
  railOpen: boolean;
  setRailOpen: (open: boolean | ((open: boolean) => boolean)) => void;
  railToggle: RefObject<HTMLButtonElement | null>;
  versions: { open: boolean; close: () => unknown };
}): RoomLayout {
  const [label] = useState(windowLabel);
  const [layout, setLayout] = useState<Layout>(() => readLayout(label));
  const wide = useRailShown();
  const papersToggle = useRef<HTMLButtonElement>(null);
  const keep = (next: Layout) => {
    writeLayout(label, next);
    setLayout(next);
  };
  /** Whether the focus is inside the room's part `name` (its id), or inside what `selector` names. */
  const holds = (name: string, selector?: string) => {
    const at = document.activeElement;
    if (at === null) return false;
    return (
      document.getElementById(room.id(name))?.contains(at) === true ||
      (selector !== undefined && at.closest(selector) !== null)
    );
  };
  const toggleRail = () => {
    // the rail's place holds Versions' timeline: the toggle closes it, so the column ends as the toggle said
    if (versions.open) versions.close();
    if (!wide) {
      setRailOpen((open) => !open);
      return;
    }
    if (layout.sidebar && holds("rail", ".ver-panel")) railToggle.current?.focus();
    keep({ ...layout, sidebar: !layout.sidebar });
  };
  const togglePapers = () => {
    if (layout.papers && holds("pane")) papersToggle.current?.focus();
    keep({ ...layout, papers: !layout.papers });
  };
  const openRail = () => {
    if (wide && !layout.sidebar) keep({ ...layout, sidebar: true });
    else setRailOpen(true);
  };
  const sidebarShown = wide ? layout.sidebar : railOpen;
  // the Mac's View menu says Hide or Show by what a press would do; the desktop does nothing off the Mac
  const tell = mac ? transport.layoutMenu : undefined;
  useEffect(() => {
    if (tell === undefined) return;
    tell
      .call(transport, sidebarShown, layout.papers)
      .catch((error: unknown) => console.warn(`set_layout_menu failed: ${String(error)}`));
  }, [tell, transport, sidebarShown, layout.papers]);
  return {
    railBeside: wide && layout.sidebar,
    rail: { shown: sidebarShown, toggle: toggleRail },
    papers: {
      shown: layout.papers,
      onToggle: togglePapers,
      controls: room.id("pane"),
      ref: papersToggle,
    },
    openRail,
    attrs: {
      ...(layout.sidebar ? {} : { "data-rail": "hidden" as const }),
      ...(layout.papers ? {} : { "data-wall": "hidden" as const }),
    },
  };
}
