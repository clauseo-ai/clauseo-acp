// A conversation's context menu's and a matter's menu's two items (U16b), loaded later: Snooze (until…), opening the
// snooze menu's times beside it, and Mark as done (Mark all done).
import { Menu as BaseMenu } from "@base-ui/react/menu";
import type { ReactNode } from "react";
import { useLayer } from "../ui/layer-context.ts";
import { MenuItem } from "../ui/menu.tsx";
import { raised, row } from "../ui/recipes-overlays.ts";
import { IconFold, IconSnooze, IconTick } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { tw } from "../lib/words-triage.ts";

/** A context menu's or a matter menu's two items: Snooze (until…), opening the times beside it, and Mark (all) done. */
export function TriageItems({
  times,
  onDone,
  matter,
}: {
  /** The snooze menu's times. */
  times: ReactNode;
  onDone: () => void;
  matter: boolean;
}) {
  const { lang } = useDisplay();
  const layer = useLayer();
  const snooze = tw(lang, matter ? "Snooze until…" : "Snooze");
  return (
    <>
      <BaseMenu.SubmenuRoot>
        <BaseMenu.SubmenuTrigger className={row} label={snooze}>
          <IconSnooze size={16} className="flex-none text-muted" />
          <span className="flex-1 deva-nudge">{snooze}</span>
          <IconFold size={14} className="flex-none text-muted" />
        </BaseMenu.SubmenuTrigger>
        <BaseMenu.Portal container={layer}>
          <BaseMenu.Positioner sideOffset={4} collisionPadding={8} className="z-20">
            <BaseMenu.Popup className={raised({ width: "menu" })}>{times}</BaseMenu.Popup>
          </BaseMenu.Positioner>
        </BaseMenu.Portal>
      </BaseMenu.SubmenuRoot>
      <MenuItem
        label={tw(lang, matter ? "Mark all done" : "Mark as done")}
        icon={<IconTick size={16} />}
        onClick={onDone}
      />
    </>
  );
}
