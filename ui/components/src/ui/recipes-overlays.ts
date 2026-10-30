// The class recipes of the raised planes that open over the work: a menu, a select's
// list, a dialog and its scrim, a toast. They are the in-hand plane: the surface tone, a lit top edge and the in-hand
// shadow, or a hairline in lite. No outline, no glass, no blur. Every value is a token class.
import { cva } from "class-variance-authority";

/** A raised plane opening from a control. 14 px corners round 8 px rows on a 6 px inset, so the corners nest. */
export const raised = cva(
  [
    "max-h-available overflow-y-auto p-s1-5 rounded-card bg-surface text-primary shadow-e-hand edge-lite outline-none",
    "motion-menu",
  ],
  {
    variants: {
      // a menu is at least the rail's width and at most an excerpt card's; a select's list is its trigger's width
      width: { menu: "min-w-menu max-w-card-tile", select: "min-w-anchor max-w-card-tile" },
    },
    defaultVariants: { width: "menu" },
  },
);

/** A row in a menu or a select's list. Highlighted, by pointer or keys, it takes the hover fill at once: keys never
 * wait on a fade. Focus draws the 2 px outline inside the plane's 6 px inset. */
export const row = [
  "flex w-full items-center gap-s3 min-h-control-small px-s3 py-s1-5 rounded-small",
  "text-ui font-ui text-primary text-left cursor-default select-none",
  "data-highlighted:bg-hover is-active:bg-press data-disabled:text-faint data-disabled:bg-transparent",
].join(" ");

/** A row's second line, such as what removing a matter leaves alone. */
export const rowNote = "block text-small text-muted in-data-disabled:text-faint";

/** A group's label in a menu, set as the rail's date groups are: 13 px, 116%, quiet. */
export const groupLabel = "px-s3 pt-s1 pb-s0-5 text-small stretch-voice text-quiet select-none";

/** The control a select opens from: a field you set, so it's drawn as the text field is, set in on the well tone
 * with nothing drawn round it; its chevron and its words say what it is. Open, it takes the surface tone, as a
 * focused field does. */
export const selectTrigger = [
  "group inline-flex items-center justify-between gap-s2 h-control max-w-full px-s3 rounded-control",
  "bg-well set-in-control text-primary text-ui font-ui whitespace-nowrap motion-colour",
  "is-hover:bg-well-strong is-active:bg-well-strong data-popup-open:bg-surface",
  "is-disabled:bg-well-strong is-disabled:text-faint is-disabled:shadow-none",
].join(" ");

/** The scrim under a dialog: the ground at 82%, a flat tone that dims, with no blur. */
export const scrim = "fixed inset-0 z-30 bg-scrim motion-scrim";

/** A dialog's plane: the in-hand plane with the long shadow of a card off the wall. */
export const dialogPlane =
  "p-s6 rounded-card bg-surface text-primary shadow-hand-card edge-lite outline-none";
/** Where a dialog stands and how it arrives: centred in the window, 440 px, rising 8 px. */
export const dialogPlace = "dialog-place z-30 motion-dialog";
/** Its title, in the matter title's voice: 22 px, 116%, 560. */
export const dialogTitle = "text-title font-chrome stretch-voice tracking-title text-primary";
/** Its body, at the loose interface size cards use, muted. */
export const dialogBody = "mt-s2 text-ui-loose text-muted";
/** Its actions, at the foot, the one that acts last. */
export const dialogActions = "mt-s6 flex flex-wrap justify-end gap-s2";

/** Where toasts stand: the window's foot, clear of a phone's toolbar, at the right on a wide window and across it on
 * a narrow one. */
export const toastViewport =
  "fixed z-40 toast-place flex flex-col items-stretch narrow:items-end gap-s2 outline-none";
/** A toast: a short raised plane, the check and the words, and Close. */
export const toast = [
  "flex items-center gap-s3 max-w-card-ref min-h-control-large pl-s4 pr-s1-5 py-s1-5 rounded-card",
  "bg-surface text-primary text-ui font-ui shadow-e-hand edge-lite motion-toast",
].join(" ");
