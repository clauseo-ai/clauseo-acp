// The class recipes of the settings' controls (switch, checkbox, radio), tabs, the text
// field and its states, progress and the separator. Off, a control is set into its plane like a field; on, it fills
// with primary, the one dark fill, under the Ask button's lit top edge. Green stays for what is checked or supports,
// red for the other side: neither marks a control's state or an error. Every value is a token class.

/** A switch's track: set in with its lip when off, primary when on. 36 by 20, scaling with the text size. */
export const switchTrack = [
  "inline-flex flex-none items-center w-switch-w h-switch-h p-s0-5 rounded-full motion-colour",
  // off, the track lightens under the pointer rather than darkening, so the thumb keeps 3:1 against it
  "bg-inset set-in-lip is-hover:bg-well-strong is-active:bg-inset",
  "data-checked:bg-primary data-checked:shadow-icon-primary",
  "is-hover:data-checked:bg-primary-hover is-active:data-checked:bg-primary-press",
  "is-disabled:bg-well-strong is-disabled:shadow-none is-disabled:data-checked:bg-faint",
].join(" ");
/** Its thumb: a raised disc in the lip's tone while off, so off reads at 3:1 against the track as well as on does;
 * on-primary once on, travelling the track. */
export const switchThumb = [
  "block size-switch-thumb rounded-full bg-edge-control shadow-e1 motion-thumb",
  "data-checked:bg-on-primary data-disabled:bg-surface data-disabled:shadow-none",
].join(" ");

/** A checkbox: an 18 px square set into its plane with its lip, primary with an on-primary tick once checked. */
export const checkBox = [
  "inline-grid flex-none place-items-center size-check rounded-label text-on-primary motion-control",
  // under the pointer it lightens, as a switch's track does; the lip holds 3:1 against the plane either way
  "bg-inset set-in-lip is-hover:bg-well-strong is-active:bg-inset is-active:press-small",
  "data-checked:bg-primary data-checked:shadow-icon-primary",
  "is-hover:data-checked:bg-primary-hover is-active:data-checked:bg-primary-press",
  "is-disabled:bg-well-strong is-disabled:shadow-none is-disabled:data-checked:bg-faint",
].join(" ");

/** A radio: an 18 px circle set into its plane with its lip, primary with an on-primary dot once chosen. */
export const radio = [
  "inline-grid flex-none place-items-center size-check rounded-full motion-control",
  "bg-inset set-in-lip is-hover:bg-well-strong is-active:bg-inset is-active:press-small",
  "data-checked:bg-primary data-checked:shadow-icon-primary",
  "is-hover:data-checked:bg-primary-hover is-active:data-checked:bg-primary-press",
  "is-disabled:bg-well-strong is-disabled:shadow-none is-disabled:data-checked:bg-faint",
].join(" ");
export const radioDot = "block size-check-dot rounded-full bg-on-primary";

/** A settings row: what it is, in the interface's words, and one line on what it does. Disabled, the label fades
 * with its control and the line keeps its contrast, so the reason stays readable. */
export const settingLabel = "block text-ui font-ui text-primary in-data-disabled:text-faint";
export const settingNote = "block mt-s0-5 text-small text-muted";

/** Tabs: the sections of a screen, on the bare ground as the rail's matters are. The chosen tab stands raised on the
 * tile-hover tone with e0, as the current matter does, on a plane that moves to the next tab. */
export const tabList = "relative isolate flex flex-wrap gap-s1 max-w-full";
export const tab = [
  "inline-flex items-center h-control px-s3 rounded-tile text-ui font-ui text-muted whitespace-nowrap motion-colour",
  "is-hover:bg-ground-hover is-hover:text-primary is-active:bg-ground-press",
  "data-active:text-primary is-hover:data-active:bg-transparent is-active:data-active:bg-transparent",
  "is-disabled:text-faint",
].join(" ");
export const tabPlane = "tab-plane -z-10 rounded-tile bg-tile-hover shadow-e0";

/** A text field on the work's surface: set in on the well tone, as the composer is, with nothing drawn round it;
 * its label above and its words say what it is. Focused it takes the surface tone and the 2 px outline round the
 * whole field. */
export const textControl = [
  "block w-full min-w-0 px-s3 rounded-control border-0 appearance-none",
  "bg-well set-in-control text-ui font-ui text-primary motion-colour",
  "placeholder:text-muted is-hover:bg-well-strong is-focus:bg-surface",
  "data-invalid:text-muted",
  "is-disabled:bg-well-strong is-disabled:text-faint is-disabled:shadow-none disabled:placeholder:text-faint",
].join(" ");
export const fieldLabel = "block text-ui font-ui text-primary in-data-disabled:text-faint";
export const fieldNote = "text-small text-muted";
/** An error stays in concrete: primary words at the chip's weight, never red, which means the other side. */
export const fieldError = "text-small font-chip text-primary";

/** A determinate progress track: primary on the inset tone, 4 px, as the plan meter is. */
export const progressTrack =
  "relative block w-full h-meter-h overflow-hidden rounded-meter bg-inset shadow-set-in-lights";
export const progressFill = "block h-full rounded-meter bg-primary";

/** The one drawn division: 1 px of the divide tone, on a whole pixel, kept in lite where shadows go. */
export const separator = {
  horizontal: "block flex-none w-full h-px bg-divide-mid",
  vertical: "block flex-none self-stretch w-px bg-divide-mid",
} as const;
