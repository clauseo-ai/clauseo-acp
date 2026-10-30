// The window's own controls. On the Mac the window keeps its corner lights; on Windows
// the toolbar is the title bar and ends in minimise, maximise or restore, and close, in Windows 11's own metrics and
// glyphs, because the platform's chrome follows the platform. No Mica: the toolbar stays on the ground.
// In the app each press calls the window (getCurrentWindow().minimize(), toggleMaximize(), close()); in a browser
// it logs what it would do. Snap Layouts on hovering maximise needs the host's hit-testing: WM_NCHITTEST answering
// HTMAXBUTTON over this button.
import { useState } from "react";
import { cn } from "../lib/cn.ts";
import type { Platform } from "../lib/display.ts";
import { useUiWords } from "../lib/words-ui.ts";

/** A caption button: 46 by 32, flush with the window's top edge, a quiet fill under the pointer as Windows draws it.
 * Focus draws its ring inside, since outside it would fall past the window's edge. */
const caption = [
  "inline-grid place-items-center flex-none w-caption-w h-caption-h bg-transparent text-primary motion-colour",
  "is-hover:bg-hover is-active:bg-press is-focus:-outline-offset-4",
].join(" ");
/** Close turns Windows' own red, with a white glyph. */
const close =
  "is-hover:bg-win-close is-hover:text-on-colour is-active:bg-win-close-press is-active:text-on-colour";

/** A glyph in Windows' own cut: 10 by 10, a 1 px line. */
function Glyph({ d }: { d: string }) {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 10 10"
      className="block size-caption-glyph overflow-visible"
      fill="none"
      stroke="currentColor"
      strokeWidth={1}
      vectorEffect="non-scaling-stroke"
    >
      <path d={d} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

const MINIMISE = "M0 5H10";
const MAXIMISE =
  "M1.5 0.5H8.5A1 1 0 0 1 9.5 1.5V8.5A1 1 0 0 1 8.5 9.5H1.5A1 1 0 0 1 0.5 8.5V1.5A1 1 0 0 1 1.5 0.5Z";
const RESTORE =
  "M1.5 2.5H6.5A1 1 0 0 1 7.5 3.5V8.5A1 1 0 0 1 6.5 9.5H1.5A1 1 0 0 1 0.5 8.5V3.5A1 1 0 0 1 1.5 2.5Z" +
  "M2.5 2.5V1.5A1 1 0 0 1 3.5 0.5H8.5A1 1 0 0 1 9.5 1.5V6.5A1 1 0 0 1 8.5 7.5H7.5";
const CLOSE = "M0.5 0.5L9.5 9.5M9.5 0.5L0.5 9.5";

/** Says what the window would do, where there's no window to do it. */
const would = (what: string) => console.info(`Clauseo would ${what} the window.`);

/** Minimise, maximise or restore, and close, at the toolbar's right on Windows. */
export function CaptionButtons({
  startMaximised = false,
  only = "all",
  className,
}: {
  /** Whether the window starts maximised, so the middle button restores. */
  startMaximised?: boolean;
  /** Which buttons the window has, where it isn't a main window: an installer has minimise and close, Windows' own
   * message box close alone. All three by default. */
  only?: "all" | "minimise-close" | "close";
  className?: string;
}) {
  const w = useUiWords();
  const [maximised, setMaximised] = useState(startMaximised);
  const maxLabel = maximised ? w.ui("Restore") : w.f("Maximise");
  return (
    <div role="group" aria-label={w.f("Window")} className={cn("room-caption flex", className)}>
      {only !== "close" && (
        <button
          type="button"
          className={caption}
          aria-label={w.f("Minimise")}
          title={w.f("Minimise")}
          onClick={() => would("minimise")}
        >
          <Glyph d={MINIMISE} />
        </button>
      )}
      {only === "all" && (
        <button
          type="button"
          className={caption}
          aria-label={maxLabel}
          title={maxLabel}
          onClick={() => {
            would(maximised ? "restore" : "maximise");
            setMaximised(!maximised);
          }}
        >
          <Glyph d={maximised ? RESTORE : MAXIMISE} />
        </button>
      )}
      <button
        type="button"
        className={cn(caption, close)}
        aria-label={w.ui("Close")}
        title={w.ui("Close")}
        onClick={() => would("close")}
      >
        <Glyph d={CLOSE} />
      </button>
    </div>
  );
}

/** The Mac's corner lights: three 12 px circles in the lights tone, set in. They stand for the window's own. */
export function CornerLights() {
  return (
    <div className="room-lights" aria-hidden="true" data-tauri-drag-region>
      <i />
      <i />
      <i />
    </div>
  );
}

export type { Platform };
