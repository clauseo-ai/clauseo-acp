// The agent's model or effort, chosen from the status line: a menu that opens upward from the value's own words. Its
// own chunk, since only a status line that offers the agent's settings needs it. A real menu: Enter, Space or the
// arrows open it, the arrows move through the values, a typed letter jumps, Enter picks and closes, Escape closes with
// focus back on the trigger. The values' names and descriptions are the agent's own.
import { Menu as BaseMenu } from "@base-ui/react/menu";
import { IconMore, IconTick } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useLayer } from "../ui/layer-context.ts";
import { groupLabel, raised, row, rowNote } from "../ui/recipes-overlays.ts";
import type { ConfigMenu } from "./status-line.tsx";

/** "Opus" on the status line, which opens the model's values. At rest it is the status line's own words with a small
 * chevron; under the pointer, focused or open it takes the plan's quiet fill. Its name says what it sets and what it
 * stands at: "Model: Opus". */
export function ConfigMenuTrigger({ menu }: { menu: ConfigMenu }) {
  const layer = useLayer();
  const name = menu.options.find((o) => o.value === menu.value)?.name ?? menu.value;
  return (
    <BaseMenu.Root>
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
          </BaseMenu.Popup>
        </BaseMenu.Positioner>
      </BaseMenu.Portal>
    </BaseMenu.Root>
  );
}
