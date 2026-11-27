// Where an item is on, from its list_addons fields (stage D2a: `scope`, `switched`, `locked`),
// and what each switch may do (library L2). The head's lens says whose switches the rows show: a
// matter's (set_addon_on, that matter only) or every matter's (set_addon_scope). A built-in skill
// takes no change (addon.built_in); a built-in tool takes a matter's switch only; under the
// firm's lock a row with scope "matter" still takes a matter's switch, one with scope "all" is
// on everywhere and takes none (settings.locked).
import type { Lang } from "@clauseo/components/lib/display.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { AddonChange, AddonView } from "./addons.ts";

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

export type { AddonChange } from "./addons.ts";

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

/** Whether the item is turned off itself (a skill pack off, a tool not enabled, the browser or the
 * connectors off in Settings), so on in no matter, switch or not: its `enabled` (D5), and for an
 * added item with scope "all", its `on` read without a matter, which says the same. */
export function ownOff(a: AddonView): boolean {
  if (builtInSkill(a) || lockedOn(a) || a.nowhere) return false;
  return a.enabled === false || (a.source === "added" && a.scope === "all" && !a.on);
}

/** Whether the item is on in `matter`: as the host resolved it for the matter open last; else
 * scope "all" (and turned on) unless switched off there, scope "matter" only where switched on. */
export function onIn(a: AddonView, matter: MatterId): boolean {
  if (lockedOn(a) || builtInSkill(a)) return true;
  if (ownOff(a)) return false;
  if (a.here_on?.matter === matter) return a.here_on.on;
  const switched = (a.switched ?? []).includes(matter);
  return a.scope === "all" ? a.on && !switched : switched;
}

/** An item turned off itself: where it was turned off, and for an added one that a switch here
 * turns it back on (the host turns on an item on nowhere when a matter or every matter asks for
 * it). A built-in tool is turned on in Settings only. */
export function ownOffLine(lang: Lang, a: AddonView, where: "row" | "detail"): string {
  const section = everywhereSection(a);
  if (section === "privacy")
    return lb(
      lang,
      "Turned off in Settings › Privacy, so no matter uses it. Turn it on there first.",
    );
  if (section === "connectors")
    return lb(
      lang,
      "Connectors are turned off in Settings › Connectors, so no matter uses it. Turn them on there first.",
    );
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

/** Whether the item is on for the lens: in its matter, or on in every matter by default. */
export function onFor(a: AddonView, lens: Lens): boolean {
  if (lens !== "all") return onIn(a, lens.matter);
  return a.scope === "all" && (lockedOn(a) || (a.on && !ownOff(a)));
}

/** Where it is on, as the row says it: "Every matter", "Every matter but 1", "Only Sharma v. State
 * of Delhi", "Only 2 matters", or turned off. */
export function scopeSummary(
  lang: Lang,
  a: AddonView,
  names: ReadonlyMap<MatterId, string>,
): string {
  const switched = switchedOf(a, names);
  if (a.nowhere) return lb(lang, "Not used in any matter");
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
export function switchFor(a: AddonView, lens: Lens): SwitchState {
  if (builtInSkill(a)) return null;
  // a built-in tool turned off is turned on in Settings, not here
  const offInSettings = a.source === "built_in" && ownOff(a);
  if (!usable(a) || offInSettings) return { checked: false, disabled: true, firm: false };
  const checked = onFor(a, lens);
  if (lockedOn(a)) return { checked: true, disabled: true, firm: true };
  if (lens !== "all") return { checked, disabled: false, firm: false };
  if (a.locked) return { checked, disabled: true, firm: true };
  // a built-in tool takes only a matter's switch (addon.built_in without one)
  return { checked, disabled: a.source === "built_in", firm: false };
}

/** The command a switch sends: with a matter, that matter's switch; with every matter, the item on
 * in every matter (scope "all", no switches) or off in every matter (scope "matter", no switches),
 * one set_addon_on with `matter` left out (D5). */
export function switchChanges(a: AddonView, lens: Lens, on: boolean): AddonChange[] {
  const { kind, id } = a;
  if (lens !== "all") return [{ set_addon_on: { kind, id, matter: lens.matter, on } }];
  return [{ set_addon_on: { kind, id, on } }];
}

/** The row as the host will list it once `change` is done: shown while it is with the host, put
 * back when the host refuses it. */
export function afterChange(a: AddonView, change: AddonChange): AddonView {
  // the matter's resolved state is the host's to say again: the held row reads its fields
  const { here_on: _, nowhere: __, ...b } = a;
  if ("set_addon_scope" in change) {
    const scope = change.set_addon_scope.scope;
    const switched = scope === "all" ? [] : [scope.matter];
    return { ...b, scope, on: true, enabled: true, switched };
  }
  if ("remove_addon" in change) return a;
  const { matter, on } = change.set_addon_on;
  // every matter: on is scope all; off is on in no matter, its scope the host's to say: the row
  // keeps its scope and reads "Not used in any matter" while held
  if (matter === undefined)
    return on
      ? { ...b, scope: "all", on: true, enabled: true, switched: [] }
      : { ...b, on: false, switched: [], nowhere: true };
  // on in a matter while on nowhere: turned on, scope matter, on in that matter only
  const nowhere =
    a.nowhere || ownOff(a) || (a.scope === "all" && !a.on && (a.switched ?? []).length === 0);
  if (on && nowhere)
    return { ...b, scope: { matter }, on: true, enabled: true, switched: [matter] };
  const differs = a.scope === "all" ? !on : on;
  const rest = (a.switched ?? []).filter((m) => m !== matter);
  return { ...b, switched: differs ? [...rest, matter] : rest };
}
