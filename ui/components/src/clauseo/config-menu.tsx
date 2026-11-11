// The agent's model or effort, chosen from the status line: a menu that opens upward from the value's own words. Its
// own chunk, since only a status line that offers the agent's settings needs it. A real menu: Enter, Space or the
// arrows open it on the value ticked now (the first with none ticked), the arrows move through the values, a typed
// letter jumps, Enter picks and closes, Escape closes with focus back on the trigger. The values' names and
// descriptions are the agent's own; a row's quiet note after its name, "(default)", is the room's.
import { Menu as BaseMenu } from "@base-ui/react/menu";
import { useRef } from "react";
import { IconMore, IconTick } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useDisplay } from "../lib/display.ts";
import { useLayer } from "../ui/layer-context.ts";
import { groupLabel, raised, row, rowNote } from "../ui/recipes-overlays.ts";
import type { ConfigMenu } from "./status-line.tsx";

/** "Opus" on the status line, which opens the model's values. At rest it is the status line's own words with a small
 * chevron; under the pointer, focused or open it takes the plan's quiet fill. Its name says what it sets and what it
 * stands at: "Model: Opus". */
export function ConfigMenuTrigger({ menu }: { menu: ConfigMenu }) {
  const layer = useLayer();
  const { lang } = useDisplay();
  const name = menu.options.find((o) => o.value === menu.value)?.name ?? menu.value;
  // opened by keys, the menu highlights its first value; the ticked one takes the focus in its place, so Enter keeps
  // the value as it is and the arrows move from it. A pointer's open highlights nothing, and nothing moves.
  const byKeys = useRef(false);
  return (
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
        if (ticked instanceof HTMLElement && ticked !== item) ticked.focus({ preventScroll: true });
      }}
    >
      <BaseMenu.Trigger
        className="room-plan room-pick"
        aria-label={`${menu.label}: ${name}`}
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
          <BaseMenu.Popup className={cn(raised({ width: "menu" }), "room-pick-menu")}>
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
                      <span className="block">
                        <span className="as-en">{o.name}</span>
                        {o.note !== undefined && (
                          <span lang={lang} className="text-muted">
                            {" "}
                            {o.note}
                          </span>
                        )}
                      </span>
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
          </BaseMenu.Popup>
        </BaseMenu.Positioner>
      </BaseMenu.Portal>
    </BaseMenu.Root>
  );
}

/** Whether a menu opened by keys: a key, or a button's click with no press behind it (Enter or Space on it). */
function fromKeys(event: Event | undefined): boolean {
  if (event === undefined) return false;
  if (event.type.startsWith("key")) return true;
  return event.type === "click" && (event as MouseEvent).detail === 0;
}
