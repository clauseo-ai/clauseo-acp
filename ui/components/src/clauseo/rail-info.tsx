// An explanation behind an info icon on the rail (U16b): the icon, named with the words, which also show under the
// pointer and on focus, on a target of at least 24 px (`.room-info`, triage.css). Loaded with the open shelf and the open
// Done fold, it stands in the fold's heading row after its name (Fold's slot), and its words open to the rail's side,
// over the work, so they cover no row.
import { use } from "react";
import { createPortal } from "react-dom";
import { Tooltip } from "../ui/tooltip.tsx";
import { IconAbout } from "../icons/icons.tsx";
import { FoldInfo } from "./rail-fold-info.ts";

export function RailInfo({ words }: { words: string }) {
  const slot = use(FoldInfo);
  if (slot === null) return null;
  return createPortal(
    <Tooltip content={words} side="right" anchor={slot.parentElement ?? undefined}>
      <button type="button" className="room-info" aria-label={words}>
        <IconAbout size={14} />
      </button>
    </Tooltip>,
    slot,
  );
}
