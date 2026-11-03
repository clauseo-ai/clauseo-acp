// The icon of a step in the thread, by what it does: ACP's tool kinds (read, edit, delete, move, search, execute,
// think, fetch, switch_mode, and other for the rest), the icon pass's glyphs (icons/tool-kinds.tsx, through
// icons.tsx), and the break for a step that failed. Everything draws a step's icon through ToolKindIcon.
import {
  IconBreak,
  IconToolDelete,
  IconToolEdit,
  IconToolExecute,
  IconToolFetch,
  IconToolMove,
  IconToolOther,
  IconToolRead,
  IconToolSearch,
  IconToolSwitchMode,
  IconToolThink,
} from "../icons/icons.tsx";
import type { IconProps } from "../icons/icon.tsx";
import { stepKindOf } from "./tool-kinds.ts";

/** A tool call's icon, by its kind (other for a kind it doesn't know); `failed`: the break, whatever its kind. */
export function ToolKindIcon({
  kind,
  failed = false,
  ...props
}: IconProps & { kind: string; failed?: boolean }) {
  if (failed) return <IconBreak {...props} />;
  // switch_mode counts as other in the words, but has a glyph of its own
  if (kind === "switch_mode") return <IconToolSwitchMode {...props} />;
  switch (stepKindOf(kind)) {
    case "read":
      return <IconToolRead {...props} />;
    case "edit":
      return <IconToolEdit {...props} />;
    case "delete":
      return <IconToolDelete {...props} />;
    case "move":
      return <IconToolMove {...props} />;
    case "search":
      return <IconToolSearch {...props} />;
    case "execute":
      return <IconToolExecute {...props} />;
    case "think":
      return <IconToolThink {...props} />;
    case "fetch":
      return <IconToolFetch {...props} />;
    case "other":
      return <IconToolOther {...props} />;
  }
}
