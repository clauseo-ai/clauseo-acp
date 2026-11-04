// Snooze and Done on the rail (U16b), drawn with it: a conversation's two buttons at its line's end under the pointer
// or with focus (Snooze, the alarm clock; Mark as done, the tick), the same in its context menu and in a matter's menu
// (Snooze until…, Mark all done); the line that stands in a row's place after either is rail-line.tsx's. The snooze
// menu's times, and the two menu items (rail-triage-items.tsx), are the room's to draw, loaded later: `menu` draws the
// times as the menu opens, `items` the items as a context menu or a matter's menu opens.
import type { ReactNode } from "react";
import { Button } from "../ui/button.tsx";
import { Menu, MenuPopup, MenuTrigger } from "../ui/menu.tsx";
import { IconSnooze, IconTick } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { tw } from "../lib/words-triage.ts";

/** A row's snooze and done: the snooze menu's content, drawn as the menu opens; a context menu's or a matter menu's
 * two items (`matter`: Snooze until… and Mark all done), drawn as it opens; and Mark as done pressed. */
export type Triage = {
  menu: () => ReactNode;
  items: (matter: boolean) => ReactNode;
  onDone: () => void;
};

/** A conversation's two buttons at its line's end, shown under the pointer or with focus; named with its title. */
export function RowTriage({ triage, title }: { triage: Triage; title: string }) {
  const { lang } = useDisplay();
  const snooze = tw(lang, "Snooze");
  const done = tw(lang, "Mark as done");
  return (
    <span className="room-conv-acts">
      <Menu>
        <MenuTrigger
          render={
            <Button kind="quiet" size="small" shape="icon" className="data-popup-open:bg-press" />
          }
          aria-label={`${snooze}, ${title}`}
          title={snooze}
          data-testid="snooze"
        >
          <IconSnooze size={16} />
        </MenuTrigger>
        <MenuPopup align="start">{triage.menu()}</MenuPopup>
      </Menu>
      <Button
        kind="quiet"
        size="small"
        shape="icon"
        aria-label={`${done}, ${title}`}
        title={done}
        data-testid="mark-done"
        onClick={triage.onDone}
      >
        <IconTick size={16} />
      </Button>
    </span>
  );
}
