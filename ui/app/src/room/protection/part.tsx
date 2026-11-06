// UC: the room's lazy protection part, one component for the first load to place (note-load.ts's LazyPart): the Not protected
// note, or the firm's notice.
import type { ComponentProps } from "react";
import { ProtectionNote } from "./note.tsx";
import { RequiredNotice } from "./required.tsx";

export function ProtectionPart(
  props:
    | ({ part: "note" } & ComponentProps<typeof ProtectionNote>)
    | ({ part: "required" } & ComponentProps<typeof RequiredNotice>),
) {
  return props.part === "note" ? <ProtectionNote {...props} /> : <RequiredNotice {...props} />;
}
