// The plan to ask with, chosen from the status line: a select that opens upward from the plan's words. Its own
// chunk, since only a status line that offers a choice needs it.
import { Select as BaseSelect } from "@base-ui/react/select";
import { useLayer } from "../ui/layer-context.ts";
import { raised, row } from "../ui/recipes-overlays.ts";
import { IconGoodLaw } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useThreadWords } from "../lib/words-thread.ts";
import { Say } from "./say.tsx";
import type { PlanChoice } from "./status-line.tsx";

/** "Uses your Claude plan" on the status line, which opens the plan to ask with. At rest it is the status line's
 * own words, unchanged; under the pointer or focused it takes a quiet fill. */
export function PlanTrigger<V extends string>({
  plan,
  choice,
}: {
  plan: string;
  choice: PlanChoice<V>;
}) {
  const w = useThreadWords();
  const layer = useLayer();
  return (
    <BaseSelect.Root
      items={choice.options}
      value={choice.value}
      onValueChange={(next) => {
        const chosen = choice.options.find((o) => o.value === next);
        if (chosen !== undefined) choice.onValueChange(chosen.value);
      }}
    >
      <BaseSelect.Trigger
        className="room-status-plan room-plan"
        aria-label={`${plan}. ${w.ui("Plan to ask with")}`}
      >
        <Say>{plan}</Say>
      </BaseSelect.Trigger>
      <BaseSelect.Portal container={layer}>
        <BaseSelect.Positioner
          alignItemWithTrigger={false}
          positionMethod="fixed"
          side="top"
          align="start"
          sideOffset={6}
          collisionPadding={8}
          className="z-20"
        >
          <BaseSelect.Popup className={raised({ width: "select" })}>
            <BaseSelect.List>
              {choice.options.map((o) => (
                <BaseSelect.Item
                  key={o.value}
                  value={o.value}
                  label={o.label}
                  className={cn(row, "justify-between")}
                >
                  <BaseSelect.ItemText className="deva-nudge">{o.label}</BaseSelect.ItemText>
                  <BaseSelect.ItemIndicator className="flex-none text-primary">
                    <IconGoodLaw size={16} />
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
