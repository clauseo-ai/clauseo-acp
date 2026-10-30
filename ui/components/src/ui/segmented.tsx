import { Toggle } from "@base-ui/react/toggle";
import { ToggleGroup } from "@base-ui/react/toggle-group";
import { cn } from "../lib/cn.ts";
import { segItem, segTrack } from "./recipes.ts";

export type SegmentOption<V extends string> = {
  value: V;
  label: string;
  /** The option's own language, when it names one: English | हिन्दी. */
  lang?: "en" | "hi";
  disabled?: boolean;
};

/** The segmented switch: Day | Night, English | हिन्दी. One side is always chosen; arrow keys move between them. */
export function SegmentedSwitch<V extends string>({
  label,
  value,
  options,
  onValueChange,
  className,
  disabled = false,
}: {
  /** Names the switch for assistive technology, in the room's words. */
  label: string;
  value: V;
  options: readonly SegmentOption<V>[];
  onValueChange: (value: V) => void;
  className?: string;
  disabled?: boolean;
}) {
  return (
    <ToggleGroup
      aria-label={label}
      value={[value]}
      disabled={disabled}
      onValueChange={(next) => {
        // a press on the chosen side leaves it chosen: the switch never empties
        const chosen = options.find((o) => o.value === next[0]);
        if (chosen !== undefined) onValueChange(chosen.value);
      }}
      className={cn(segTrack, className)}
    >
      {options.map((o) => (
        <Toggle
          key={o.value}
          value={o.value}
          lang={o.lang}
          disabled={o.disabled ?? false}
          className={cn(segItem, o.lang === "hi" ? "font-hi" : o.lang === "en" ? "font-en" : "")}
        >
          <span className="deva-nudge">{o.label}</span>
        </Toggle>
      ))}
    </ToggleGroup>
  );
}
