// The window's own controls. On the Mac the window keeps its corner lights, drawn by macOS over CornerLights' place;
// on Windows the toolbar is the title bar and ends in minimise, maximise or restore, and close, in Windows 11's own
// metrics and glyphs, because the platform's chrome follows the platform. No Mica: the toolbar stays on the ground.
// This is a part: it knows no host. The app passes the handlers and the window's maximised state (ui/app's
// useWindowFrame); without them each press logs what it would do. The host's hit-testing finds the maximise button by
// data-caption="maximise" and answers HTMAXBUTTON over it, so Windows shows Snap Layouts; while it does, a native
// window covers the button and the page gets no :hover, so the host sets data-pointer="hover" or "press" on it instead.
// For the same reason the maximise button's title never shows as a tooltip there; Snap Layouts takes its place, and the
// native window carries Windows' own name for it, for screen readers.
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
/** The maximise button under the host's pointer, where the page sees none (Snap Layouts' cover): the same fills. */
const hostPointer = "host-hover:bg-hover host-press:bg-press";
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

/** On the half pixel, as the other glyphs are, so the 1 px line lands on one row of pixels at 100%. */
const MINIMISE = "M0 5.5H10";
const MAXIMISE =
  "M1.5 0.5H8.5A1 1 0 0 1 9.5 1.5V8.5A1 1 0 0 1 8.5 9.5H1.5A1 1 0 0 1 0.5 8.5V1.5A1 1 0 0 1 1.5 0.5Z";
const RESTORE =
  "M1.5 2.5H6.5A1 1 0 0 1 7.5 3.5V8.5A1 1 0 0 1 6.5 9.5H1.5A1 1 0 0 1 0.5 8.5V3.5A1 1 0 0 1 1.5 2.5Z" +
  "M2.5 2.5V1.5A1 1 0 0 1 3.5 0.5H8.5A1 1 0 0 1 9.5 1.5V6.5A1 1 0 0 1 8.5 7.5H7.5";
const CLOSE = "M0.5 0.5L9.5 9.5M9.5 0.5L0.5 9.5";

/** Says what the window would do, where there's no window to do it. */
const would = (what: string) => console.info(`Clauseo would ${what} the window.`);

/** The window, as the app wires the caption buttons to it (ui/app's useWindowFrame): its maximised state and the three
 * presses, each optional. */
export type CaptionWiring = {
  /** Optional: whether the window is maximised, as the window says. Overrides the button's own state. */
  maximised?: boolean;
  /** Optional: minimises the window. */
  onMinimise?: () => void;
  /** Optional: maximises the window, or restores it when maximised. */
  onToggleMaximise?: () => void;
  /** Optional: closes the window. */
  onClose?: () => void;
};

/** Minimise, maximise or restore, and close, at the toolbar's right on Windows. Each press calls its handler where
 * one is given and otherwise logs what it would do, button by button. The middle button follows `maximised` where it
 * is given (Restore while true, Maximise while false); without it the button keeps its own state, starting at
 * `startMaximised` and flipping on each press, handler or not. */
export function CaptionButtons({
  startMaximised = false,
  maximised: shown,
  onMinimise,
  onToggleMaximise,
  onClose,
  only = "all",
  className,
}: {
  /** Whether the window starts maximised, so the middle button restores. */
  startMaximised?: boolean;
  /** Which buttons the window has, where it isn't a main window: an installer has minimise and close, Windows' own
   * message box close alone. All three by default. */
  only?: "all" | "minimise-close" | "close";
  className?: string;
} & CaptionWiring) {
  const w = useUiWords();
  const [own, setOwn] = useState(startMaximised);
  const maximised = shown ?? own;
  const maxLabel = maximised ? w.ui("Restore") : w.f("Maximise");
  return (
    <div role="group" aria-label={w.f("Window")} className={cn("room-caption flex", className)}>
      {only !== "close" && (
        <button
          type="button"
          className={caption}
          aria-label={w.f("Minimise")}
          title={w.f("Minimise")}
          onClick={onMinimise ?? (() => would("minimise"))}
        >
          <Glyph d={MINIMISE} />
        </button>
      )}
      {only === "all" && (
        <button
          type="button"
          className={cn(caption, hostPointer)}
          data-caption="maximise"
          aria-label={maxLabel}
          title={maxLabel}
          onClick={() => {
            if (onToggleMaximise === undefined) would(maximised ? "restore" : "maximise");
            else onToggleMaximise();
            if (shown === undefined) setOwn(!maximised);
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
        onClick={onClose ?? (() => would("close"))}
      >
        <Glyph d={CLOSE} />
      </button>
    </div>
  );
}

/** The Mac's corner lights: three 12 px circles in the lights tone, set in. They stand for the window's own. Where
 * `native` is set, macOS draws its own traffic lights in this very place (the host puts them there), so the element
 * keeps its size and its drag region, and paints no circles (shell.css, .room-lights[data-native]). */
export function CornerLights({ native = false }: { native?: boolean }) {
  return (
    <div
      className="room-lights"
      aria-hidden="true"
      // deep: Tauri drags only from the element pressed, and the gaps between the native lights fall on the circles
      data-tauri-drag-region="deep"
      data-native={native ? "" : undefined}
    >
      <i />
      <i />
      <i />
    </div>
  );
}

export type { Platform };
