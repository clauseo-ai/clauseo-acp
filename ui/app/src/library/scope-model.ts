// Where an item is on, from its list_addons fields (stage D2a: `scope`, `switched`, `locked`),
// and what each switch may do (library L2). The head's lens says whose switches the rows show: a
// matter's (set_addon_on, that matter only) or every matter's (set_addon_scope). A built-in skill
// takes no change (addon.built_in); a built-in tool takes a matter's switch only; under the
// firm's lock a row with scope "matter" still takes a matter's switch, one with scope "all" is
// on everywhere and takes none (settings.locked).
import type { Lang } from "@clauseo/components/lib/display.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { AddonScope, AddonView } from "./addons.ts";

/** Whose switches the rows show: one matter's, or every matter's. */
export type Lens = { matter: MatterId } | "all";

/** The Settings section that turns a built-in tool off everywhere: the browser's switch is in
 * Privacy, under On the web; Case Law goes with the connectors' switch, in Connectors. */
export function everywhereSection(a: AddonView): "privacy" | "connectors" | null {
  if (a.kind !== "tool" || a.source !== "built_in") return null;
  return a.id === "browser" ? "privacy" : a.id === "case-law" ? "connectors" : null;
}

/** A built-in tool's line while the switches are every matter's, where its switch is greyed. */
export function everywhereLine(lang: Lang, a: AddonView): string {
  const section = everywhereSection(a);
  if (section === "privacy")
    return lb(
      lang,
      "Comes with Clauseo. Switch it off for a matter from that matter, or turn it off everywhere in Settings › Privacy.",
    );
  if (section === "connectors")
    return lb(
      lang,
      "Comes with Clauseo. Switch it off for a matter from that matter, or turn connectors off everywhere in Settings › Connectors.",
    );
  return lb(lang, "Comes with Clauseo, so it stays. Turn it off where you don't want it.");
}

/** What one change asks of the host. */
export type AddonChange =
  | { set_addon_on: { kind: AddonView["kind"]; id: string; matter: MatterId; on: boolean } }
  | { set_addon_scope: { kind: AddonView["kind"]; id: string; scope: AddonScope } }
  | { remove_addon: { kind: AddonView["kind"]; id: string } };

export const builtInSkill = (a: AddonView) => a.kind === "skill" && a.source === "built_in";
/** Whether a switch can move: nothing in the item's way (Case Law out of credit still switches). */
export const usable = (a: AddonView) => a.status === "ready" || a.status === "needs_credit";
/** Under the firm's lock with scope "all": on everywhere, whatever a matter stored. */
export const lockedOn = (a: AddonView) => a.locked && a.scope === "all";

/** The matters the host says differ from the scope's default, those the room knows. */
function switchedOf(a: AddonView, names: ReadonlyMap<MatterId, string>): MatterId[] {
  if (lockedOn(a)) return [];
  const ids = a.switched ?? [];
  return names.size === 0 ? ids : ids.filter((id) => names.has(id));
}

/** Whether an added item is turned off itself (a skill pack off, a tool not enabled), so on in no
 * matter, switch or not (the host's rule, addons/change.rs). Scope "all": its `on` read without a
 * matter. Scope "matter": the read for the matter open last, where it is switched on, says off. */
export function ownOff(a: AddonView): boolean {
  if (a.source !== "added" || lockedOn(a)) return false;
  if (a.scope === "all") return !a.on;
  const here = a.here_on;
  return here !== undefined && (a.switched ?? []).includes(here.matter) && !here.on;
}

/** Whether the item is on in `matter`: as the host resolved it for the matter open last; else
 * scope "all" (and turned on) unless switched off there, scope "matter" only where switched on. */
export function onIn(a: AddonView, matter: MatterId): boolean {
  if (lockedOn(a) || builtInSkill(a)) return true;
  if (a.here_on?.matter === matter) return a.here_on.on;
  if (ownOff(a)) return false;
  const switched = (a.switched ?? []).includes(matter);
  return a.scope === "all" ? a.on && !switched : switched;
}

/** An added item turned off itself: where it was turned off, and that a switch here turns it back
 * on (the host turns on an item on nowhere when a matter or a scope asks for it). */
export function ownOffLine(lang: Lang, a: AddonView, where: "row" | "detail"): string {
  if (where === "row")
    return a.kind === "tool"
      ? lb(
          lang,
          "Turned off in Settings › Tools, so no matter uses it. Switch it on here to turn it back on.",
        )
      : lb(
          lang,
          "Turned off in Settings › Skills, so no matter uses it. Switch it on here to turn it back on.",
        );
  return a.kind === "tool"
    ? lb(
        lang,
        "Turned off in Settings › Tools, so no matter uses it. Choose where to use it to turn it back on.",
      )
    : lb(
        lang,
        "Turned off in Settings › Skills, so no matter uses it. Choose where to use it to turn it back on.",
      );
}

/** Why a row's switch is greyed under every matter with no matter at all: off everywhere names one. */
export const needsMatter = (a: AddonView, lens: Lens, matters: number): boolean =>
  lens === "all" &&
  matters === 0 &&
  a.source === "added" &&
  !a.locked &&
  usable(a) &&
  onFor(a, lens);

/** Whether the item is on for the lens: in its matter, or on in every matter by default. */
export function onFor(a: AddonView, lens: Lens): boolean {
  return lens === "all" ? a.scope === "all" && (a.on || lockedOn(a)) : onIn(a, lens.matter);
}

/** Where it is on, as the row says it: "Every matter", "Every matter but 1", "Only Sharma v. State
 * of Delhi", "Only 2 matters", or turned off. */
export function scopeSummary(
  lang: Lang,
  a: AddonView,
  names: ReadonlyMap<MatterId, string>,
): string {
  const switched = switchedOf(a, names);
  if (ownOff(a)) return lb(lang, "Turned off");
  if (a.scope === "all") {
    if (!a.on && !lockedOn(a) && !builtInSkill(a)) return lb(lang, "Turned off");
    return switched.length === 0
      ? lb(lang, "Every matter")
      : lb(lang, "Every matter but {n}", { n: switched.length });
  }
  // scope matter, on in no matter: never "Only <matter>"
  if (switched.length === 0) return lb(lang, "Not used in any matter");
  const one = switched.length === 1 ? names.get(switched[0]!) : undefined;
  return one !== undefined
    ? lb(lang, "Only {matters}", { matters: one })
    : lb(lang, "Only {n} matters", { n: switched.length });
}

/** What a row's switch is: none (a built-in skill), or its state, and the firm's word when the firm
 * holds it. */
export type SwitchState = null | { checked: boolean; disabled: boolean; firm: boolean };

/** The row's switch for the lens. Something in the item's way leaves it off and greyed; a built-in
 * tool's scope and a locked row's scope are not the lawyer's to change. */
export function switchFor(a: AddonView, lens: Lens, matters: number): SwitchState {
  if (builtInSkill(a)) return null;
  if (!usable(a)) return { checked: false, disabled: true, firm: false };
  const checked = onFor(a, lens);
  if (lockedOn(a)) return { checked: true, disabled: true, firm: true };
  if (lens !== "all") return { checked, disabled: false, firm: false };
  if (a.locked) return { checked, disabled: true, firm: true };
  // every matter off needs a matter to name: set_addon_scope always turns the item on in one
  const disabled = a.source === "built_in" || (checked && matters === 0);
  return { checked, disabled, firm: false };
}

/** The commands a switch sends. With a matter, that matter's switch. With every matter, on is
 * scope "all"; off is scope `matter` in `first` and then off there, so it is on nowhere and new
 * matters start without it (set_addon_scope alone always leaves it on in one matter).
 * TODO(stage D D5): off everywhere becomes one command, `{"set_addon_on": {kind, id, on: false}}`
 * with `matter` left out (every matter: scope matter, no switches; on: true is scope all, no
 * switches); it answers done then addons_changed, settings.locked for a locked item. Swap the two
 * commands below for it once D5 lands. */
export function switchChanges(
  a: AddonView,
  lens: Lens,
  on: boolean,
  first: MatterId | null,
): AddonChange[] {
  const { kind, id } = a;
  if (lens !== "all") return [{ set_addon_on: { kind, id, matter: lens.matter, on } }];
  if (on) return [{ set_addon_scope: { kind, id, scope: "all" } }];
  if (first === null) return [];
  return [
    { set_addon_scope: { kind, id, scope: { matter: first } } },
    { set_addon_on: { kind, id, matter: first, on: false } },
  ];
}

/** The row as the host will list it once `change` is done: shown while it is with the host, put
 * back when the host refuses it. */
export function afterChange(a: AddonView, change: AddonChange): AddonView {
  // the matter's resolved state is the host's to say again: the held row reads its fields
  const { here_on: _, ...b } = a;
  if ("set_addon_scope" in change) {
    const scope = change.set_addon_scope.scope;
    return { ...b, scope, on: true, switched: scope === "all" ? [] : [scope.matter] };
  }
  if ("remove_addon" in change) return a;
  const { matter, on } = change.set_addon_on;
  // on in a matter while on nowhere: turned on, scope matter, on in that matter only
  const nowhere = ownOff(a) || (a.scope === "all" && !a.on && (a.switched ?? []).length === 0);
  if (on && nowhere) return { ...b, scope: { matter }, on: true, switched: [matter] };
  const differs = a.scope === "all" ? !on : on;
  const rest = (a.switched ?? []).filter((m) => m !== matter);
  return { ...b, switched: differs ? [...rest, matter] : rest };
}
