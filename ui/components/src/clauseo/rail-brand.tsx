// Clauseo's lockup at the head of the rail's column (rail-brand.css): on the Mac in the toolbar's lead, after the
// traffic lights and on the matter title's line; on Windows and Linux, and wherever the rail folds away, at the top of
// the rail, above New matter. The room draws both and the width and the platform show one. Not a control. Where it
// stands in the title bar it drags the window, as the lead around it does.
import type { Platform } from "../lib/display.ts";
import { Lockup } from "./lockup.tsx";

/** The lockup, in the toolbar's lead (`lead`) or at the top of the rail (`rail`). */
export function RailBrand({ at, platform }: { at: "lead" | "rail"; platform: Platform }) {
  return (
    <div
      className="room-brand"
      data-at={at}
      data-platform={platform}
      // deep: Tauri drags only from the element pressed, and a press lands on the lockup's paths
      {...(at === "lead" ? { "data-tauri-drag-region": "deep" } : {})}
    >
      <Lockup className="room-brand-lockup" />
    </div>
  );
}
