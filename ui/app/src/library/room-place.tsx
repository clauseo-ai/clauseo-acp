// The room's side of Skills and connectors: whether the place stands in the centre (and where it
// opened), its row on the rail's foot, and the place itself, loaded the first time it opens. The
// place closes when a matter or a conversation opens from the rail, or a new matter is made (the
// room calls close). The list's source starts with the room, so the rail's count and the MCP
// servers' catch-up are heard from the first frame.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import { useState, type ReactNode } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import type { AgentKind, MatterId } from "../transport/protocol.ts";
import { addonsSourceOf, useAddons } from "./addons-source.ts";
import { LibraryRow } from "./library-entry.tsx";
import { LibraryLazy } from "./library-lazy.tsx";
import { markSeen, wasSeen } from "./seen.ts";
import type { LibraryStart } from "./library.tsx";

export type LibraryPlace = {
  /** Whether the place stands in the centre. */
  shown: boolean;
  /** The title bar's words while it does. */
  title: string;
  /** Opens it, at a part and an item when given. */
  show: (start?: LibraryStart) => void;
  /** Closes it: a matter or a conversation opened. */
  close: () => void;
  /** Its row, for the rail's foot. */
  row: ReactNode;
  /** The place, for the centre; null while it is closed. */
  centre: ReactNode;
};

export function useLibraryPlace({
  transport,
  names,
  plan,
  onShow,
}: {
  transport: HostTransport;
  names: ReadonlyMap<MatterId, string>;
  plan: AgentKind | undefined;
  /** Heard each time it opens: a folded rail folds. */
  onShow: () => void;
}): LibraryPlace {
  const { lang } = useDisplay();
  const [source] = useState(() => addonsSourceOf(transport));
  const { items } = useAddons(source);
  const [start, setStart] = useState<LibraryStart | null>(null);
  const [seen, setSeen] = useState(wasSeen);
  const show = (where: LibraryStart = {}) => {
    setStart(where);
    onShow();
    if (!seen) {
      markSeen();
      setSeen(true);
    }
  };
  const shown = start !== null;
  const listed = items?.filter((a) => a.id !== "agent-browser") ?? null;
  const count =
    listed === null
      ? ""
      : lb(lang, "{skills} skills, {connectors} connectors", {
          skills: listed.filter((a) => a.kind === "skill").length,
          connectors: listed.filter((a) => a.kind === "tool").length,
        });
  return {
    shown,
    title: lb(lang, "Skills and connectors"),
    show,
    close: () => setStart(null),
    row: <LibraryRow note={count} current={shown} fresh={!seen} onOpen={() => show()} />,
    centre:
      start !== null ? (
        <LibraryLazy
          transport={transport}
          source={source}
          names={names}
          plan={plan}
          start={start}
        />
      ) : null,
  };
}
