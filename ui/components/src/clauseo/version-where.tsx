// Where the versions live. The Versions panel's foot says it in one line, "The matter's versions are kept on this
// computer.", and a press on it says the rest: in Clauseo's own folder, as the computer writes it, never in OneDrive
// or any folder that syncs; the matter's own folder syncs as it always has; another computer starts afresh; how much
// room it takes, kept for good, with the way to keep 90 days instead in Settings.
import { Popover as BasePopover } from "@base-ui/react/popover";
import { Button } from "../ui/button.tsx";
import { IconSafeWorkspace } from "../icons/icons.tsx";
import type { Platform } from "../lib/display.ts";
import { useVersionWords } from "../lib/words-versions.ts";
import { FloatingCard } from "./floating-card.tsx";
import { Heading } from "./heading.tsx";
import { Say } from "./say.tsx";

export function VersionWhere({
  platform,
  folder,
  size,
  settings,
  open,
  onOpenChange,
  onShow,
}: {
  platform: Platform;
  /** Clauseo's own folder for the matter's versions, as the computer writes it. */
  folder: string;
  /** How much room this matter's versions take: "412 MB". */
  size: string;
  /** Where Settings keeps how long versions are kept. */
  settings: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Shows the folder in Finder or File Explorer. */
  onShow?: () => void;
}) {
  const w = useVersionWords();
  return (
    <BasePopover.Root open={open} onOpenChange={onOpenChange}>
      <BasePopover.Trigger className="ver-foot">
        <IconSafeWorkspace size={14} className="ver-foot-icon" />
        <span>
          <Say>{w.v("The matter's versions are kept on this computer.")}</Say>
        </span>
      </BasePopover.Trigger>
      <FloatingCard side="top" width="history" className="ver-where">
        <BasePopover.Title render={<Heading level={2} className="allows-title" />}>
          {w.v("Where versions are kept")}
        </BasePopover.Title>
        <p className="ver-where-line">
          <Say>
            {w.v(
              "On this computer, in Clauseo's own folder. Never in OneDrive, Dropbox or any folder that syncs, so syncing never touches them.",
            )}
          </Say>
        </p>
        <p className="ver-where-path" lang="en">
          {folder}
        </p>
        <p className="ver-where-line">
          <Say>
            {w.v("The matter's own folder can sync as it always has. Its versions stay here.")}
          </Say>
        </p>
        <p className="ver-where-line">
          <Say>{w.v("On another computer, this matter's versions start afresh.")}</Say>
        </p>
        <p className="ver-where-line ver-where-size">
          <Say>{w.v("This matter's versions take {size}. They are kept for good.", { size })}</Say>
        </p>
        <div className="ver-where-actions">
          <Button kind="tone" size="small" onClick={onShow}>
            <span className="deva-nudge">
              {platform === "mac" ? w.ui("Show in Finder") : w.ui("Show in File Explorer")}
            </span>
          </Button>
          <Button kind="quiet" size="small" render={<a href={settings} />} nativeButton={false}>
            <span className="deva-nudge">{w.v("Keep 90 days instead")}</span>
          </Button>
        </div>
      </FloatingCard>
    </BasePopover.Root>
  );
}
