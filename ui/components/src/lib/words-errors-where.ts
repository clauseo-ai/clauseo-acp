// A refused turn's words where the refusal names a place (R17 PR-E, Windows' tier B): `sandbox.pool_owned` and
// `sandbox.cant_label` carry the item in TurnFailed's `at` (a path in the matter's folder), and cant_label the kind of
// folder in its `detail`. `{where}` is the path as the app shows a matter's path, set apart (FSI…PDI) so a
// right-to-left mark in it can't reorder the Hindi around it. Every other code says errorWords' sentence.
import type { Lang } from "./display.ts";
import { wordsOf } from "./words-core.ts";
import { errorWords, ownershipLine, whereOf, type AgentName } from "./words-errors.ts";

const own = wordsOf("errors");

/** What a refused turn names beside its code: the item (`at`) and the detail, either left out when not known. The
 * view's StepEntry carries `failure_at` only, never the detail. */
export type FailurePlace = { at?: string | null | undefined; detail?: string | null | undefined };

/** The cant_label kinds the words tell apart; every other (State, ClaudeStore, CredentialLink, a newer one) reads as
 * the Safe workspace not prepared. */
export type LabelKind = "matter" | "notes" | "other";

/** The kind a cant_label names: its detail's first word (Matter, Notes, ... in any case), else Matter when the turn
 * names an item in the matter's folder, else other. */
export function labelKindOf(place: FailurePlace): LabelKind {
  const word = /^\s*([a-z_]+)/i.exec(place.detail ?? "")?.[1]?.toLowerCase() ?? null;
  if (word === "matter") return "matter";
  if (word === "notes") return "notes";
  if (word === null || word === "") return hasAt(place) ? "matter" : "other";
  // a kind the sandbox names that isn't one of the two, or a detail that is no kind: by the item alone
  return KNOWN_OTHER.has(word) || !hasAt(place) ? "other" : "matter";
}

const KNOWN_OTHER = new Set([
  "state",
  "claudestore",
  "claude_store",
  "credentiallink",
  "credential_link",
]);

const hasAt = (place: FailurePlace): place is { at: string } =>
  typeof place.at === "string" && place.at !== "";

/** A refused turn's words: the code's sentence, or, for pool_owned and cant_label, the one that names where. */
export function failureWords(
  lang: Lang,
  code: string,
  place: FailurePlace = {},
  agent?: AgentName,
): string {
  if (code === "sandbox.pool_owned" && hasAt(place))
    return `${own(lang, "{where} in this matter still belongs to the Safe workspace, so Clauseo can't answer here.", { where: whereOf(place.at) })} ${ownershipLine(lang, place.at)}`;
  if (code === "sandbox.cant_label") {
    const kind = labelKindOf(place);
    if (kind === "matter" && hasAt(place))
      return own(
        lang,
        "The Safe workspace couldn't be given access to {where}, so Clauseo can't answer here. If it keeps happening, ask your IT team.",
        { where: whereOf(place.at) },
      );
    if (kind === "notes")
      return own(
        lang,
        "The Safe workspace couldn't be given access to this matter's notes, so Clauseo can't answer here. If it keeps happening, ask your IT team.",
      );
    return own(
      lang,
      "The Safe workspace couldn't be prepared for this matter, so Clauseo can't answer here. If it keeps happening, ask your IT team.",
    );
  }
  return errorWords(lang, code, agent);
}
