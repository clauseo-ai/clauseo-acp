// A web tool's glyph with its state on it (DESIGN.md §Icons, icons first): on as drawn, off dimmed and slashed, a
// firm's lock dimmed, its lower right corner given to the lock. The words for the state go in the tooltip and the accessible name.
import { useId } from "react";
import { Icon } from "./icon.tsx";
import { IconBrowser, IconWebsite } from "./icons.tsx";

export type ToolGlyphState = "on" | "off" | "locked";

export type ToolGlyphProps = {
  /** web: web search, the globe; browser: Clauseo's own browser, the window. */
  glyph: "web" | "browser";
  state: ToolGlyphState;
  /** 14 in a row, 16 in a menu row or a Settings section, 11 in a chip. */
  size?: 11 | 14 | 16;
  /** Names the glyph and its state for assistive technology; without it the glyph is hidden, as an Icon is. */
  label?: string;
  className?: string;
};

/** Twice the stroke's half and the 1.5 px gap either side: the knockout's width around the slash, in screen px. */
const KNOCKOUT = 4.5;

/** The lock IconSafeWorkspace draws, scaled to about 11.5 units tall and set in the glyph's lower right corner, which
 * is knocked out under it, so it reads at 14 px: its body's sides land near x.75, on the pixel grid at 1x. */
const LOCK = "translate(6.8 6) scale(0.7)";
const LOCK_BODY = "M2.75 9.25h14.5v9h-14.5z";
const LOCK_SHACKLE = "M6 9.25V6a4 4 0 0 1 8 0v3.25";

export function ToolGlyph({ glyph, state, size = 14, label, className }: ToolGlyphProps) {
  const mask = `tool-glyph-${useId().replace(/[^\w-]/g, "")}`;
  const Glyph = glyph === "web" ? IconWebsite : IconBrowser;
  // the glyph's own svg nests at the full 20 grid; its stroke stays 1.5 px, as it never scales
  const shape = <Glyph size={20} />;
  return (
    <Icon
      size={size}
      {...(label === undefined ? {} : { label })}
      {...(className === undefined ? {} : { className })}
    >
      {state === "on" ? (
        shape
      ) : (
        <>
          <defs>
            <mask id={mask} maskUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
              <rect x="0" y="0" width="20" height="20" fill="white" stroke="none" />
              {state === "off" ? (
                <path
                  data-part="knockout"
                  d="M2.75 2.75l14.5 14.5"
                  stroke="black"
                  strokeWidth={KNOCKOUT}
                  vectorEffect="non-scaling-stroke"
                />
              ) : (
                <g data-part="knockout" transform={LOCK}>
                  <path
                    d={LOCK_BODY}
                    fill="black"
                    stroke="black"
                    strokeWidth={KNOCKOUT}
                    vectorEffect="non-scaling-stroke"
                  />
                  <path
                    d={LOCK_SHACKLE}
                    stroke="black"
                    strokeWidth={KNOCKOUT}
                    vectorEffect="non-scaling-stroke"
                  />
                </g>
              )}
            </mask>
          </defs>
          <g mask={`url(#${mask})`} className="text-muted">
            {shape}
          </g>
          {state === "off" ? (
            <path
              data-part="slash"
              className="text-muted"
              d="M2.75 2.75l14.5 14.5"
              vectorEffect="non-scaling-stroke"
            />
          ) : (
            <g data-part="lock" transform={LOCK}>
              <path d={LOCK_BODY} vectorEffect="non-scaling-stroke" />
              <path d={LOCK_SHACKLE} vectorEffect="non-scaling-stroke" />
            </g>
          )}
        </>
      )}
    </Icon>
  );
}
