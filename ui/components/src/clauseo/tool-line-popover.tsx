// The tool line's popover shell: the status line's quiet button holding the tools' glyphs, named and tooltipped in
// words (DESIGN.md §Icons), and the raised plane it opens upward, as the plan's select does (plan-trigger.tsx). Its
// glyphs and rows are the caller's. Loaded only with the tool line's popover chunk (the app's room/tool-popover.tsx),
// which is why it builds on Base UI's popover here rather than on ui/popover.tsx: that one is History's, and sharing it
// would split a chunk off both.
import { Popover as BasePopover } from "@base-ui/react/popover";
import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import { Progress as BaseProgress } from "@base-ui/react/progress";
import { useId, useRef, type ReactElement, type ReactNode } from "react";
import { useLayer } from "../ui/layer-context.ts";
import { cn } from "../lib/cn.ts";
import { progressFill, progressTrack } from "../ui/recipes-controls.ts";
import { raised } from "../ui/recipes-overlays.ts";
import { tip } from "../ui/recipes.ts";
import { IconDownload } from "../icons/icons.tsx";

/** The tool line's tooltip, as ui/tooltip.tsx's, raised over the status line's thread and over the popover's plane:
 * the shared one stacks in the page's layer, under both. On the line's button, and on the popover's buttons whose
 * words are only a tooltip (Open Settings' reach, the download's size, Try again). */
export function LineTip({ content, children }: { content: ReactNode; children: ReactElement }) {
  const layer = useLayer();
  return (
    <BaseTooltip.Root>
      <BaseTooltip.Trigger render={children} delay={500} />
      <BaseTooltip.Portal container={layer}>
        <BaseTooltip.Positioner
          side="top"
          sideOffset={8}
          collisionPadding={8}
          positionMethod="fixed"
          className="z-30"
        >
          <BaseTooltip.Popup className={tip}>{content}</BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}

export function ToolLinePopover({
  label,
  glyphs,
  children,
}: {
  /** Where each tool stands, in words: the button's accessible name and its tooltip. */
  label: string;
  /** The tools' glyphs, hidden from assistive technology: the label says them. */
  glyphs: ReactNode;
  children: ReactNode;
}) {
  const layer = useLayer();
  const trigger = useId();
  // the popup itself takes the focus on open, by pointer or by key: no button stands focused or ringed (DESIGN.md's
  // Quiet Start), and an Enter right after opening presses nothing, Download Chrome above all; Tab reaches the buttons
  const popup = useRef<HTMLDivElement>(null);
  return (
    <BasePopover.Root>
      <LineTip content={label}>
        <BasePopover.Trigger
          id={trigger}
          aria-label={label}
          className="room-plan room-tool-glyphs"
          data-testid="tool-line"
        >
          {glyphs}
        </BasePopover.Trigger>
      </LineTip>
      <BasePopover.Portal container={layer}>
        <BasePopover.Positioner
          positionMethod="fixed"
          side="top"
          align="start"
          sideOffset={6}
          collisionPadding={8}
          className="z-20"
        >
          <BasePopover.Popup
            ref={popup}
            initialFocus={popup}
            tabIndex={-1}
            aria-labelledby={trigger}
            // one width in every state, the widest the menu takes, so the plane never jumps as the download moves on
            className={`${raised({ width: "menu" })} w-card-tile`}
            data-testid="tool-popover"
          >
            {children}
          </BasePopover.Popup>
        </BasePopover.Positioner>
      </BasePopover.Portal>
    </BasePopover.Root>
  );
}

/** The Chrome download's bar in one line: the download glyph, the bar, and how far in words (DESIGN.md §Icons), which
 * are the bar's value for screen readers too. Determinate only: nothing spins, and the fill never moves at rest. */
export function DownloadBar({
  value,
  max,
  text,
  name,
  className,
}: {
  value: number;
  max: number;
  /** How far: "64 of 201 MB", or "Unpacking". */
  text: string;
  /** The bar's name: "Download Chrome". */
  name: string;
  className?: string;
}) {
  return (
    <BaseProgress.Root
      value={value}
      max={max}
      getAriaValueText={() => text}
      aria-label={name}
      className={cn("flex items-center gap-s2 text-small text-muted", className)}
      data-testid="tool-progress"
    >
      <IconDownload size={14} />
      <BaseProgress.Track className={cn(progressTrack, "min-w-0 flex-1")}>
        <BaseProgress.Indicator className={progressFill} />
      </BaseProgress.Track>
      <span className="flex-none text-primary" data-testid="tool-progress-text">
        {text}
      </span>
    </BaseProgress.Root>
  );
}
