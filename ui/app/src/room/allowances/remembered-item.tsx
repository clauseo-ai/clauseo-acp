// R18: the matter menu's "Remembered answers…" (menu.tsx), shown while the open matter has any.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { aw } from "@clauseo/components/lib/words-allowances.ts";
import { MenuItem } from "@clauseo/components/ui/menu.tsx";
import { useHeader, useWatch } from "../../store/hooks.ts";
import type { ViewStore } from "../../store/store.ts";
import type { MatterId } from "../../transport/protocol.ts";
import { matterStreamOf } from "../matter-stream.ts";

/** "Remembered answers…", while the matter has any: its header, on the matter's stream, held only while the menu
 * is open (a menu's items mount with its popup), so the rail holds no stream of its own. */
export function RememberedItem({
  views,
  matter,
  lang,
  onOpen,
}: {
  views: ViewStore;
  matter: MatterId;
  lang: Lang;
  onOpen: () => void;
}) {
  const stream = matterStreamOf(matter);
  useWatch(views, stream);
  const header = useHeader(views, stream);
  const any =
    header !== null &&
    "matter_header" in header &&
    (header.matter_header?.allowances?.length ?? 0) > 0;
  return any ? <MenuItem label={aw(lang, "Remembered answers…")} onClick={onOpen} /> : null;
}
