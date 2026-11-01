// The dev-only frame page: the room's 64 px title bar alone, on the room's ground, to try the window's own frame
// before the room exists. Its header is placed as RoomToolbar places its own. Deleted when the room lands.
import { CaptionButtons, CornerLights } from "@clauseo/components/clauseo/caption.tsx";
import { cn } from "@clauseo/components/lib/cn.ts";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import type { WindowControls } from "../transport/window-controls.ts";
import { useWindowFrame } from "./use-window-frame.ts";

/** RoomToolbar's placing of the caption buttons: flush with the window's top right. */
const captionPlace = "self-start mid:ml-s5 max-mid:order-4 max-mid:-mt-s3 max-mid:-mr-s4";

/** The caption buttons wired to the window. */
function WiredCaptions({ controls }: { controls: WindowControls }) {
  return <CaptionButtons className={captionPlace} {...useWindowFrame(controls)} />;
}

/** The title bar and an empty room under it. Without controls (no desktop app) the caption buttons log their presses. */
export function FramePage({ controls }: { controls: WindowControls | null }) {
  const { platform } = useDisplay();
  const captions = platform !== "mac";
  return (
    <div className="room-window">
      <header
        className={cn("room-top", captions && "mid:pr-0")}
        data-platform={platform}
        data-tauri-drag-region
      >
        <div className="room-lead" data-tauri-drag-region>
          {platform === "mac" && <CornerLights native />}
        </div>
        <div className="grow self-stretch" data-tauri-drag-region />
        {captions &&
          (controls === null ? (
            <CaptionButtons className={captionPlace} />
          ) : (
            <WiredCaptions controls={controls} />
          ))}
      </header>
    </div>
  );
}
