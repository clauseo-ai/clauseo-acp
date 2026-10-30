import { Checkbox as BaseCheckbox } from "@base-ui/react/checkbox";
import { cn } from "../lib/cn.ts";
import { IconTick } from "../icons/icons.tsx";
import { checkBox } from "./recipes-controls.ts";

/** A checkbox, for a setting that is on or off and reads as a yes to a question, such as a warning to show. Set
 * into the plane when clear, primary with a tick when checked; the tick lands whole, never drawn half. Space flips
 * it. It renders a native button, so a sibling label (Field.Label, or SettingRow) names it. */
export function Checkbox({
  checked,
  onCheckedChange,
  disabled = false,
  label,
  className,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  /** Names the checkbox when no visible label is beside it. */
  label?: string;
  className?: string;
}) {
  return (
    <BaseCheckbox.Root
      nativeButton
      render={<button type="button" />}
      checked={checked}
      onCheckedChange={(next) => onCheckedChange(next)}
      disabled={disabled}
      aria-label={label}
      className={cn(checkBox, className)}
    >
      <BaseCheckbox.Indicator className="flex">
        <IconTick size={14} />
      </BaseCheckbox.Indicator>
    </BaseCheckbox.Root>
  );
}
