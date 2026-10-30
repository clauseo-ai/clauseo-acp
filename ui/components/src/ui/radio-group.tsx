import { Radio } from "@base-ui/react/radio";
import { RadioGroup as BaseRadioGroup } from "@base-ui/react/radio-group";
import type { ComponentProps } from "react";
import { cn } from "../lib/cn.ts";
import { radio, radioDot } from "./recipes-controls.ts";

/** A radio group, for one choice of a few that each need a line of explanation, such as lite mode's Automatic, On
 * and Off. The arrows move the choice; Tab moves on. Lay it out with RadioSetting, which labels each choice. */
export const RadioGroup = BaseRadioGroup;

/** One choice: an 18 px circle set into the plane, primary with an on-primary dot once chosen. A native button, so
 * a sibling label names it. */
export function RadioItem({
  value,
  disabled = false,
  className,
  ...props
}: Omit<ComponentProps<typeof Radio.Root>, "className" | "render" | "nativeButton"> & {
  value: string;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <Radio.Root
      nativeButton
      render={<button type="button" />}
      value={value}
      disabled={disabled}
      className={cn(radio, className)}
      {...props}
    >
      <Radio.Indicator className={radioDot} />
    </Radio.Root>
  );
}
