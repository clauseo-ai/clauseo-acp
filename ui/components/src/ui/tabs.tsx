import { Tabs as BaseTabs } from "@base-ui/react/tabs";
import type { ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { tab, tabList, tabPlane } from "./recipes-controls.ts";

export type TabOption<V extends string> = { value: V; label: string; disabled?: boolean };

/** The sections of one screen, such as Settings. The arrows move between tabs and choose as they go; Home and End
 * reach the ends; Tab moves on into the chosen section. */
export const Tabs = BaseTabs.Root;

/** The row of tabs, on the bare ground. The chosen one stands raised as the current matter does, on a plane that
 * moves to the next tab on the spring, at once under reduced motion. */
export function TabsList<V extends string>({
  label,
  tabs,
  className,
}: {
  /** Names the row for assistive technology: "Settings". */
  label: string;
  tabs: readonly TabOption<V>[];
  className?: string;
}) {
  return (
    <BaseTabs.List aria-label={label} activateOnFocus className={cn(tabList, className)}>
      {tabs.map((t) => (
        <BaseTabs.Tab key={t.value} value={t.value} disabled={t.disabled ?? false} className={tab}>
          <span className="deva-nudge">{t.label}</span>
        </BaseTabs.Tab>
      ))}
      <BaseTabs.Indicator className={tabPlane} />
    </BaseTabs.List>
  );
}

/** One section's content. */
export function TabsPanel({
  value,
  className,
  children,
}: {
  value: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <BaseTabs.Panel value={value} className={className}>
      {children}
    </BaseTabs.Panel>
  );
}
