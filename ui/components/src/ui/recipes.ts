// The class recipes the primitives share. Every value is a token class; variants differ by
// colour where the design says they do and nowhere else. Components import these; nothing else should.
import { cva } from "class-variance-authority";

/** One family, three kinds, three heights, plus the quiet link that turns to the draft. */
export const button = cva(
  [
    "inline-flex items-center justify-center gap-s2 whitespace-nowrap font-ui motion-control select-none",
    "is-active:press is-disabled:cursor-default",
  ],
  {
    variants: {
      kind: {
        // the Ask button, the only dark button; at night the fill turns light and the words dark
        primary: [
          "bg-primary text-on-primary shadow-ask min-w-ask",
          "is-hover:bg-primary-hover is-active:bg-primary-press is-active:shadow-ask-press",
          "is-disabled:bg-well-strong is-disabled:text-faint is-disabled:shadow-none",
        ],
        // a raised tile of the ground's own tone
        tone: [
          "bg-tile text-primary shadow-e1",
          "is-hover:bg-tile-hover is-hover:shadow-e1-hover is-active:bg-ground-hover is-active:shadow-e1-press",
          "is-disabled:bg-ground-hover is-disabled:text-faint is-disabled:shadow-none",
        ],
        // no fill until the pointer arrives
        quiet: [
          "bg-transparent text-muted",
          "is-hover:bg-hover is-hover:text-primary is-active:bg-press is-disabled:text-faint",
        ],
        // the ready hint on the status line: quiet's tones, with the hint's own geometry below
        link: [
          "bg-transparent text-muted",
          "is-hover:bg-hover is-hover:text-primary is-active:bg-press is-disabled:text-faint",
        ],
      },
      size: {
        small: "h-control-small px-s3 rounded-small text-small-row",
        default: "h-control px-s4 rounded-control text-ui",
        large: "h-control-large px-s6 rounded-tile text-ui",
      },
      shape: {
        text: "",
        icon: "px-0",
      },
    },
    compoundVariants: [
      { kind: "primary", shape: "text", className: "px-s5" },
      // the hint is 28 px on one line; when its words can't share a line, its parts wrap onto a second, never cut
      {
        kind: "link",
        className:
          "h-auto min-h-hint max-w-full flex-wrap justify-start gap-y-0 px-s2 py-s0-5 rounded-small text-small-row",
      },
      { shape: "icon", size: "small", className: "w-control-small" },
      { shape: "icon", size: "default", className: "w-control" },
      { shape: "icon", size: "large", className: "w-control-large" },
    ],
    defaultVariants: { kind: "tone", size: "default", shape: "text" },
  },
);

/** The joined pair, History | Undo: one raised segmented control on the tile tone. */
export const pair = "inline-flex h-control rounded-control bg-tile shadow-e1";
export const pairButton = [
  "inline-flex items-center gap-s1-5 h-control px-s3 text-small-row font-ui text-primary whitespace-nowrap",
  "bg-transparent motion-colour first:rounded-l-control last:rounded-r-control pair-divide",
  "is-hover:bg-tile-hover is-active:bg-ground-hover data-popup-open:bg-ground-hover",
  "is-disabled:text-faint is-disabled:bg-transparent",
].join(" ");

/** A key cap: one inner hairline, quieter than the words beside it. */
export const kbd =
  "inline-grid place-items-center min-w-kbd-w h-kbd px-s1 rounded-kbd bg-transparent text-muted ring-kbd text-small leading-none font-ui";

/** The one chip: 20 px tall, 7 px sides, 4 px corners, 12 px words at 520. Variants differ only in colour. */
export const chip = cva(
  "inline-flex items-center justify-center gap-s1 flex-none h-chip px-chip-pad rounded-label text-chip font-chip tracking-chip whitespace-nowrap align-middle",
  {
    variants: {
      role: {
        relied: "bg-primary text-on-primary",
        accepted: "bg-primary text-on-primary",
        supports: "bg-green text-on-colour",
        against: "bg-red text-on-colour",
        prepared: "bg-yellow text-on-yellow",
        good: "bg-green-tint text-green-text",
        for: "bg-neutral-tint text-muted",
        cite: "bg-red-tint text-red-text",
        checking: "bg-neutral-tint text-muted",
      },
      ground: { surface: "", tile: "" },
    },
    // on a lit tile, and in its card's band, the chip takes its tile's own colour, deeper and opaque
    compoundVariants: [
      { role: "supports", ground: "tile", className: "bg-chip-green" },
      { role: "against", ground: "tile", className: "bg-chip-red" },
      { role: "prepared", ground: "tile", className: "bg-chip-yellow" },
    ],
    defaultVariants: { ground: "surface" },
  },
);

/** The segmented switch: a track set into its ground, the chosen side raised out of it on the surface tone with e0 and
 * its words in primary. Nothing is drawn round it. */
export const segTrack = "inline-flex p-seg-pad rounded-card bg-well shadow-set-in-field";
export const segItem = [
  "h-seg px-s2 rounded-tile bg-transparent text-small font-ui text-muted whitespace-nowrap motion-control",
  "is-hover:text-primary is-active:press-small",
  "data-pressed:bg-surface data-pressed:text-primary data-pressed:shadow-e0 data-pressed:cursor-default data-pressed:transform-none",
  "is-disabled:text-faint",
].join(" ");

/** A floating card: History, an excerpt card. The surface tone, 14 px corners, the e3 shadow, a hairline in lite. */
export const floating = cva(
  "bg-surface text-primary rounded-card overflow-hidden outline-none edge-lite motion-float",
  {
    variants: {
      width: {
        history: "w-history",
        tile: "w-card-tile",
        row: "w-card-row",
        ref: "w-card-ref",
        fit: "",
      },
      // a card grown from the wall throws the in-hand shadow and the card's own; others the floating card's
      from: { wall: "shadow-hand-card", text: "shadow-e3" },
    },
    defaultVariants: { width: "fit", from: "text" },
  },
);

/** A tooltip: a small floating card, words in the primary tone. */
export const tip =
  "max-w-card-tile px-s3 py-s1-5 rounded-small bg-surface text-primary text-small font-ui shadow-e3 edge-lite motion-tip";

/** A collapsible's trigger: the "N more" strip at the wall's foot, or the line an earlier exchange folds to. */
export const foldTrigger = cva(
  "group inline-flex items-center gap-s2 font-ui text-small-row text-muted motion-colour",
  {
    variants: {
      kind: {
        more: [
          "flex w-full h-more justify-center px-s3 bg-ground rule-top-mid",
          "is-hover:bg-ground-hover is-hover:text-primary is-active:bg-ground-press",
        ],
        fold: [
          "flex-wrap gap-y-0 max-w-full min-h-hint pl-s1-5 pr-s2 -ml-s1-5 rounded-chip bg-transparent",
          "is-hover:bg-well is-hover:text-primary is-active:bg-well-strong",
        ],
      },
    },
    defaultVariants: { kind: "fold" },
  },
);

/** The search field: set into the ground on the inset tone, with nothing drawn round it; its icon and its words say
 * what it is. Focused, the surface tone and the primary outline round the whole field. */
export const searchBox = [
  "flex items-center gap-s2 w-search min-w-control h-control px-s3 rounded-control bg-inset set-in-control",
  "text-muted cursor-text motion-colour search-fold",
  "is-hover:bg-inset-hover is-focus-within:bg-surface is-focus-within:text-primary",
  "is-focus-within:outline-2 is-focus-within:outline-primary is-focus-within:outline-offset-2",
  "data-disabled:bg-ground-hover data-disabled:text-faint data-disabled:shadow-none data-disabled:cursor-default",
].join(" ");
export const searchInput = [
  // the words stand the field's whole height, on the composer's 26 px line, so a run of Latin inside Devanagari never
  // reaches past the input's edge
  "flex-1 self-stretch min-w-0 p-0 bg-transparent border-0 outline-none appearance-none text-ui leading-field text-inherit deva-nudge-field",
  "is-focus:outline-none disabled:placeholder:text-faint",
].join(" ");
