// A conversation's context menu's and a matter's menu's two items (U16b), loaded later: Snooze (Snooze until, for a
// matter), opening the snooze menu's times beside it (its chevron says so, with no "…"), and Mark as done (Mark all
// done; Mark as not done for the open conversation once done).
import { Menu as BaseMenu } from "@base-ui/react/menu";
import type { ReactNode } from "react";
import { useLayer } from "../ui/layer-context.ts";
import { MenuItem } from "../ui/menu.tsx";
import { raised, row } from "../ui/recipes-overlays.ts";
import { IconFold, IconSnooze, IconTick, IconUndo } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { tw } from "../lib/words-triage.ts";
import { twl } from "../lib/words-triage-late.ts";

export function TriageItems({
  times,
  onDone,
  matter,
  done = false,
}: {
  /** The snooze menu's times. */
  times: ReactNode;
  /** Mark (all) done pressed; with `done`, Mark as not done. */
  onDone: () => void;
  matter: boolean;
  /** The conversation is done already: its second item is Mark as not done. */
  done?: boolean;
}) {
  const { lang } = useDisplay();
  const layer = useLayer();
  const snooze = matter ? twl(lang, "Snooze until") : tw(lang, "Snooze");
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
        label={
          done ? twl(lang, "Mark as not done") : tw(lang, matter ? "Mark all done" : "Mark as done")
        }
        icon={done ? <IconUndo size={16} /> : <IconTick size={16} />}
        onClick={onDone}
      />
    </>
  );
}
