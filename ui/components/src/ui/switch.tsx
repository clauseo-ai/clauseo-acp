import { Switch as BaseSwitch } from "@base-ui/react/switch";
import { cn } from "../lib/cn.ts";
import { switchThumb, switchTrack } from "./recipes-controls.ts";

/** A switch, for a setting that is on or off and takes effect at once, such as compact rows. Off, its track is set
 * into the plane; on, it fills with primary and the thumb travels across on the spring. Space or Enter flips it.
 * It renders a native button, so a sibling label (Field.Label, or SettingRow) names it. */
export function Switch({
  checked,
  onCheckedChange,
  disabled = false,
  label,
  className,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  /** Names the switch when no visible label is beside it. */
  label?: string;
  className?: string;
}) {
  return (
    <BaseSwitch.Root
      nativeButton
      render={<button type="button" />}
      checked={checked}
      onCheckedChange={(next) => onCheckedChange(next)}
      disabled={disabled}
      aria-label={label}
      className={cn(switchTrack, className)}
    >
      <BaseSwitch.Thumb className={switchThumb} />
    </BaseSwitch.Root>
  );
}
