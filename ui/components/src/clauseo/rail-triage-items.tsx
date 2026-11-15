// A conversation's context menu's and a matter's menu's two items (U16b), loaded later: Snooze (Snooze until, for a
// matter), opening the snooze menu's times beside it (its chevron says so, with no "…"), and Mark as done (Mark all
// conversations done, for a matter; Mark as not done for the open conversation once done). The open conversation's
// menu adds Let this conversation read, opening the other matters' names beside it the same way: a press lets the
// conversation read that matter's files.
import { Menu as BaseMenu } from "@base-ui/react/menu";
import type { ReactNode } from "react";
import { useLayer } from "../ui/layer-context.ts";
import { MenuItem } from "../ui/menu.tsx";
import { raised, row } from "../ui/recipes-overlays.ts";
import { IconFold, IconFolder, IconSnooze, IconTick, IconUndo } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { twl } from "../lib/words-triage-late.ts";

/** The other matters a conversation could be let read, by name, and what a press on one does. */
export type ReadPick = {
  matters: readonly { matter: string; name: string }[];
  onPick: (matter: string) => void;
};

export function TriageItems({
  times,
  onDone,
  matter,
  done = false,
  reads,
}: {
  /** The snooze menu's times. */
  times: ReactNode;
  /** Mark as done (Mark all conversations done) pressed; with `done`, Mark as not done. */
  onDone: () => void;
  matter: boolean;
  /** The conversation is done already: its second item is Mark as not done. */
  done?: boolean;
  /** Optional: Let this conversation read, with the matters it lists; none to list, no item. */
  reads?: ReadPick | undefined;
}) {
  const { lang } = useDisplay();
  const layer = useLayer();
  const snooze = matter ? twl(lang, "Snooze until") : twl(lang, "Snooze");
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
          done
            ? twl(lang, "Mark as not done")
            : twl(lang, matter ? "Mark all conversations done" : "Mark as done")
        }
        icon={done ? <IconUndo size={16} /> : <IconTick size={16} />}
        onClick={onDone}
      />
      {reads !== undefined && reads.matters.length > 0 && <ReadItem reads={reads} />}
    </>
  );
}

/** Let this conversation read, opening the other matters' names beside it (MenuItem sets each name in its own
 * script, as the rail's names are). */
function ReadItem({ reads }: { reads: ReadPick }) {
  const { lang } = useDisplay();
  const layer = useLayer();
  const label = twl(lang, "Let this conversation read");
  return (
    <BaseMenu.SubmenuRoot>
      <BaseMenu.SubmenuTrigger className={row} label={label} data-testid="read-matter">
        <IconFolder size={16} className="flex-none text-muted" />
        <span className="flex-1 deva-nudge">{label}</span>
        <IconFold size={14} className="flex-none text-muted" />
      </BaseMenu.SubmenuTrigger>
      <BaseMenu.Portal container={layer}>
        <BaseMenu.Positioner sideOffset={4} collisionPadding={8} className="z-20">
          <BaseMenu.Popup className={raised({ width: "menu" })}>
            {reads.matters.map((m) => (
              <MenuItem key={m.matter} label={m.name} onClick={() => reads.onPick(m.matter)} />
            ))}
          </BaseMenu.Popup>
        </BaseMenu.Positioner>
      </BaseMenu.Portal>
    </BaseMenu.SubmenuRoot>
  );
}
