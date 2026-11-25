// The agent's model or effort, chosen from the status line: a menu that opens upward from the value's own words. Its
// own chunk, since only a status line that offers the agent's settings needs it. A real menu: Enter, Space or the
// arrows open it on the value ticked now (the first with none ticked), the arrows move through the values, a typed
// letter jumps, Enter picks and closes, Escape closes with focus back on the trigger. The values' names and
// descriptions are the agent's own, a description shown only when the agent gives one. A polite live region
// beside the trigger says the value as the trigger names it ("Model: Opus") each time it changes after the trigger is
// shown: a pick, or the host moving it back; the trigger's own name changing is not heard once focus is on it. A value
// that changes with the menu's scope (another conversation opened) changes silently, and empties the region. A menu
// may end in one quiet line of plain words (its foot: the model menu's "Code mode on"), which the menu's name says too.
import { Menu as BaseMenu } from "@base-ui/react/menu";
import { useEffect, useId, useRef, useState } from "react";
import { IconMore, IconTick } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useLayer } from "../ui/layer-context.ts";
import { groupLabel, raised, row, rowNote } from "../ui/recipes-overlays.ts";
import { menuName } from "./logic.ts";
import type { ConfigMenu } from "./status-line.tsx";

/** "Opus" on the status line, which opens the model's values. At rest it is the status line's own words with a small
 * chevron; under the pointer, focused or open it takes the plan's quiet fill. Its name says what it sets and what it
 * stands at: "Model: Opus". */
export function ConfigMenuTrigger({ menu }: { menu: ConfigMenu }) {
  const layer = useLayer();
  const name = menuName(menu);
  // opened by keys, the menu highlights its first value; the ticked one takes the focus in its place, so Enter keeps
  // the value as it is and the arrows move from it. A pointer's open highlights nothing, and nothing moves.
  const byKeys = useRef(false);
  const named = name === menu.label ? name : `${menu.label}: ${name}`;
  const said = useSaidOnChange(menu.scope, menu.value, named);
  const footId = useId();
  return (
    <>
      <BaseMenu.Root
        onOpenChange={(open, details) => {
          byKeys.current = open && fromKeys(details.event);
        }}
        onItemHighlighted={(item) => {
          if (!byKeys.current || item === undefined) return;
          byKeys.current = false;
          const ticked = [...(item.parentElement?.children ?? [])].find(
            (row) => row.getAttribute("aria-checked") === "true",
          );
          if (ticked instanceof HTMLElement && ticked !== item)
            ticked.focus({ preventScroll: true });
        }}
      >
        <BaseMenu.Trigger
          className="room-plan room-pick"
          aria-label={named}
          data-testid={`pick-${menu.id}`}
        >
          <bdi lang="en" className="as-en">
            {name}
          </bdi>
          <IconMore size={14} className="room-pick-chevron" />
        </BaseMenu.Trigger>
        <BaseMenu.Portal container={layer}>
          <BaseMenu.Positioner
            side="top"
            align="start"
            sideOffset={8}
            collisionPadding={8}
            className="z-20"
          >
            <BaseMenu.Popup
              className={cn(raised({ width: "menu" }), "room-pick-menu")}
              aria-describedby={menu.foot === undefined ? undefined : footId}
            >
              <BaseMenu.Group>
                <BaseMenu.GroupLabel className={cn(groupLabel, "deva-nudge")}>
                  {menu.label}
                </BaseMenu.GroupLabel>
                <BaseMenu.RadioGroup
                  value={menu.value}
                  onValueChange={(next: unknown) => {
                    if (typeof next === "string" && next !== menu.value) menu.onValueChange(next);
                  }}
                >
                  {menu.options.map((o) => (
                    <BaseMenu.RadioItem
                      key={o.value}
                      value={o.value}
                      label={o.name}
                      closeOnClick
                      className={cn(
                        row,
                        "justify-between",
                        o.description !== undefined && "items-start",
                      )}
                    >
                      <span className="min-w-0" lang="en">
                        <span className="block as-en">{o.name}</span>
                        {o.description !== undefined && (
                          <span className={cn(rowNote, "as-en")}>{o.description}</span>
                        )}
                      </span>
                      <BaseMenu.RadioItemIndicator
                        className={cn(
                          "flex-none text-primary",
                          o.description !== undefined && "mt-s0-5",
                        )}
                      >
                        <IconTick size={16} />
                      </BaseMenu.RadioItemIndicator>
                    </BaseMenu.RadioItem>
                  ))}
                </BaseMenu.RadioGroup>
              </BaseMenu.Group>
              {menu.foot !== undefined && (
                <p
                  id={footId}
                  role="none"
                  className={cn(rowNote, "room-pick-foot deva-nudge")}
                  data-testid={`pick-${menu.id}-foot`}
                >
                  {menu.foot}
                </p>
              )}
            </BaseMenu.Popup>
          </BaseMenu.Positioner>
        </BaseMenu.Portal>
      </BaseMenu.Root>
      <span className="sr-only" aria-live="polite" aria-atomic="true">
        {said}
      </span>
    </>
  );
}

/** What the live region says: nothing at first, then `words` each time `value` changes within the same `scope`; a
 * change of scope (another conversation) says nothing, and empties what was said. */
function useSaidOnChange(scope: string | undefined, value: string, words: string): string {
  const [said, setSaid] = useState("");
  const was = useRef({ scope, value });
  useEffect(() => {
    const before = was.current;
    if (before.scope === scope && before.value === value) return;
    was.current = { scope, value };
    setSaid(before.scope === scope ? words : "");
  }, [scope, value, words]);
  return said;
}

/** Whether a menu opened by keys: a key, or a button's click with no press behind it (Enter or Space on it). */
function fromKeys(event: Event | undefined): boolean {
  if (event === undefined) return false;
  if (event.type.startsWith("key")) return true;
  return event.type === "click" && (event as MouseEvent).detail === 0;
}
