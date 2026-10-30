import { Select as BaseSelect } from "@base-ui/react/select";
import type { ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { Mixed } from "../lib/mixed.tsx";
import { useLayer } from "./layer-context.ts";
import { IconTick, IconMore as IconChevron } from "../icons/icons.tsx";
import { raised, row, selectTrigger } from "./recipes-overlays.ts";

/** The select's visible name, inside its row: it names the trigger, and a press on it focuses the trigger. */
export const SelectLabel = BaseSelect.Label;

export type SelectOption<V extends string> = { value: V; label: string };

/** One choice from a short list, such as the plan to ask with. The trigger is a raised tile of the ground's tone
 * with a chevron; its list opens under it as the in-hand plane, at least as wide, the chosen row ticked. The arrows
 * move, a typed letter jumps to the row it starts, Enter or Space chooses, and Escape closes with focus back on
 * the trigger. The list never overlaps the trigger. Base UI's <style> elements, which the CSP refuses, are turned off
 * once for the page by the CSPProvider at its root. */
export function Select<V extends string>({
  label,
  value,
  options,
  onValueChange,
  disabled = false,
  describedBy,
  children,
  className,
}: {
  /** Names the select for assistive technology, in the room's words, when no SelectLabel is given as a child. */
  label?: string;
  /** The id of a line that says what the choice does. */
  describedBy?: string;
  /** What stands before the trigger on its row: a SelectLabel and its line, as a settings row. */
  children?: ReactNode;
  value: V;
  options: readonly SelectOption<V>[];
  onValueChange: (value: V) => void;
  disabled?: boolean;
  className?: string;
}) {
  const layer = useLayer();
  const trigger = (
    <BaseSelect.Trigger
      aria-label={label}
      aria-describedby={describedBy}
      className={cn(selectTrigger, className)}
    >
      <BaseSelect.Value className="deva-nudge">
        {(chosen: V) => <Mixed>{options.find((o) => o.value === chosen)?.label ?? ""}</Mixed>}
      </BaseSelect.Value>
      <BaseSelect.Icon className="flex text-muted motion-turn group-data-popup-open:rotate-180 group-data-disabled:text-faint">
        <IconChevron size={16} />
      </BaseSelect.Icon>
    </BaseSelect.Trigger>
  );
  return (
    <BaseSelect.Root
      items={options}
      value={value}
      disabled={disabled}
      onValueChange={(next) => {
        const chosen = options.find((o) => o.value === next);
        if (chosen !== undefined) onValueChange(chosen.value);
      }}
    >
      {children === undefined ? (
        trigger
      ) : (
        <div className="flex flex-wrap items-center justify-between gap-x-s4 gap-y-s2">
          {children}
          {trigger}
        </div>
      )}
      <BaseSelect.Portal container={layer}>
        <BaseSelect.Positioner
          alignItemWithTrigger={false}
          side="bottom"
          align="start"
          sideOffset={4}
          collisionPadding={8}
          className="z-20"
        >
          <BaseSelect.Popup className={raised({ width: "select" })}>
            <BaseSelect.List>
              {options.map((o) => (
                <BaseSelect.Item
                  key={o.value}
                  value={o.value}
                  label={o.label}
                  className={cn(row, "justify-between")}
                >
                  <BaseSelect.ItemText className="deva-nudge">
                    <Mixed>{o.label}</Mixed>
                  </BaseSelect.ItemText>
                  <BaseSelect.ItemIndicator className="flex-none text-primary">
                    <IconTick size={16} />
                  </BaseSelect.ItemIndicator>
                </BaseSelect.Item>
              ))}
            </BaseSelect.List>
          </BaseSelect.Popup>
        </BaseSelect.Positioner>
      </BaseSelect.Portal>
    </BaseSelect.Root>
  );
}
