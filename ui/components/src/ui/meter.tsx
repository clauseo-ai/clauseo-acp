import { Meter as BaseMeter } from "@base-ui/react/meter";
import { Children, isValidElement, type ComponentProps, type ReactNode } from "react";
import { cn } from "../lib/cn.ts";

/** How much of the plan is left: plenty, close to the limit, or the limit reached until the reset. Concrete in every
 * state, never red, which means the other side: the meter and the words say it. */
export type PlanState = "plenty" | "close" | "reached";

/** The plan meter: the plan's name in primary with a 44 by 4 px meter, primary on the inset tone, and what's left
 * beneath it in the muted tone. From close to the limit, the line turns to primary at the chip's weight, so it's
 * noticed by its words and weight without any amber; reached, the meter stands empty as well. The meter tells assistive technology the same in words. The meter's words are
 * part of the title bar, so a press on them drags the window, as the toolbar's empty space does. */
export function PlanMeter({
  name,
  value,
  valueText,
  status,
  state = "plenty",
  track = true,
  inButton = false,
  className,
}: {
  /** "Claude plan" */
  name: ReactNode;
  /** The share of the plan left, 0 to 100. */
  value: number;
  /** The value in words: "62 percent of the plan left". */
  valueText: string;
  /** "Plenty left until the 9:40 pm reset" */
  status: ReactNode;
  state?: PlanState;
  /** Optional: false where there's no share to draw, an account billed by use or a plan whose figures
   * didn't come, so an empty track never reads as nothing left. */
  track?: boolean;
  /** Optional: the meter stands inside a button, as the toolbar's plan does when it opens its card. The
   * button is named by its words, so the meter drops its role, whose value text would take the plan's name's place in
   * the button's name, and the hidden "x" Base UI adds for VoiceOver. The host says the share in words itself. */
  inButton?: boolean;
  className?: string;
}) {
  return (
    <div
      className={cn("grid text-small text-muted", className)}
      data-state={state}
      data-tauri-drag-region
    >
      <BaseMeter.Root
        value={value}
        aria-valuetext={valueText}
        className="flex items-center gap-s2"
        data-tauri-drag-region
        {...(inButton ? { render: withoutHiddenX, role: "none", "aria-valuetext": undefined } : {})}
      >
        <BaseMeter.Label className="text-primary" data-tauri-drag-region>
          {name}
        </BaseMeter.Label>
        {track && (
          <BaseMeter.Track className="relative block w-meter h-meter-h overflow-hidden rounded-meter bg-inset shadow-set-in-lights">
            <BaseMeter.Indicator className="absolute inset-y-0 left-0 rounded-meter bg-primary" />
          </BaseMeter.Track>
        )}
      </BaseMeter.Root>
      <p className={cn(state !== "plenty" && "text-primary font-chip")} data-tauri-drag-region>
        {status}
      </p>
    </div>
  );
}

/** Base UI's meter root with its own children only: the hidden "x" it appends, its last child, left out. */
function withoutHiddenX(props: ComponentProps<"div">) {
  const all = props.children;
  const kids = isValidElement<{ children?: ReactNode }>(all)
    ? Children.toArray(all.props.children).slice(0, -1)
    : all;
  return <div {...props}>{kids}</div>;
}
