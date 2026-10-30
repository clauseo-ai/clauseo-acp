import { Menu as BaseMenu } from "@base-ui/react/menu";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { Mixed } from "../lib/mixed.tsx";
import { useLayer } from "./layer-context.ts";
import { IconTick } from "../icons/icons.tsx";
import { groupLabel, raised, row, rowNote } from "./recipes-overlays.ts";
import { Separator } from "./separator.tsx";

/** A menu of actions on one thing, such as a matter's row. Click, Enter, Space or the down arrow opens it; the arrows
 * move through its rows, a typed letter jumps to the row it starts, Enter acts, and Escape or a press outside
 * closes it with focus back on the trigger. */
export const Menu = BaseMenu.Root;
export const MenuTrigger = BaseMenu.Trigger;
export const MenuGroup = BaseMenu.Group;

/** The raised plane the rows stand on: the in-hand plane, under its trigger's start edge, 4 px below, always
 * inside the window. It fades in and moves 6 px out from the trigger over 160 ms, and leaves in 90. */
export function MenuPopup({
  side = "bottom",
  align = "start",
  className,
  children,
  ...props
}: Omit<ComponentProps<typeof BaseMenu.Popup>, "className"> & {
  side?: "top" | "bottom" | "left" | "right";
  align?: "start" | "center" | "end";
  className?: string;
  children: ReactNode;
}) {
  const layer = useLayer();
  return (
    <BaseMenu.Portal container={layer}>
      <BaseMenu.Positioner
        side={side}
        align={align}
        sideOffset={4}
        collisionPadding={8}
        className="z-20"
      >
        <BaseMenu.Popup className={cn(raised({ width: "menu" }), className)} {...props}>
          {children}
        </BaseMenu.Popup>
      </BaseMenu.Positioner>
    </BaseMenu.Portal>
  );
}

/** One action. note: a second line on what it leaves alone. The label alone is matched when a letter is typed. */
export function MenuItem({
  label,
  note,
  disabled = false,
  onClick,
}: {
  label: string;
  note?: string;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <BaseMenu.Item
      label={label}
      disabled={disabled}
      className={cn(row, note !== undefined && "items-start")}
      {...(onClick === undefined ? {} : { onClick })}
    >
      <span className="min-w-0">
        <span className="block deva-nudge">
          <Mixed>{label}</Mixed>
        </span>
        {note !== undefined && (
          <span className={rowNote}>
            <Mixed>{note}</Mixed>
          </span>
        )}
      </span>
    </BaseMenu.Item>
  );
}

/** An action that stays on or off, such as pinning a matter to the top. A tick at the row's end says it's on; the
 * menu stays open so the change can be seen. */
export function MenuCheckboxItem({
  label,
  checked,
  onCheckedChange,
  disabled = false,
}: {
  label: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <BaseMenu.CheckboxItem
      label={label}
      checked={checked}
      onCheckedChange={(next) => onCheckedChange(next)}
      closeOnClick={false}
      disabled={disabled}
      className={cn(row, "justify-between")}
    >
      <span className="deva-nudge">
        <Mixed>{label}</Mixed>
      </span>
      <BaseMenu.CheckboxItemIndicator className="flex-none text-primary in-data-disabled:text-faint">
        <IconTick size={16} />
      </BaseMenu.CheckboxItemIndicator>
    </BaseMenu.CheckboxItem>
  );
}

/** A group's label, as the rail sets its date groups. lang: a matter's name stays as filed, in English. */
export function MenuGroupLabel({ children, lang }: { children: ReactNode; lang?: "en" }) {
  return (
    <BaseMenu.GroupLabel lang={lang} className={cn(groupLabel, lang === "en" && "font-en")}>
      {children}
    </BaseMenu.GroupLabel>
  );
}

/** Between groups: the one drawn division, across the plane's inset. */
export function MenuSeparator() {
  return <Separator className="my-s1-5" />;
}
