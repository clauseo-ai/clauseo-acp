// The permission prompt's answers and its keys, apart from the DOM, so the tests can hold them.

/** The four answers, as ACP's kinds: allow_once, allow_always, reject_once, reject_always. "never" has no key. */
export type PermissionAnswer = "once" | "always" | "deny" | "never";
/** Waiting for the lawyer, answered, or gone: expired, cancelled, or waiting where no answer can reach it any more. */
export type PermissionState = "pending" | PermissionAnswer | "gone";

/** The answers in ACP's kind order, the order the buttons always stand in. */
export const ANSWER_ORDER: readonly PermissionAnswer[] = ["once", "always", "deny", "never"];

/** The keys, read from a keydown: Enter, Cmd or Ctrl with Enter, Escape. Shift+Enter stays a new line, and nothing
 * counts while an input method is composing (`isComposing`, or keyCode 229) or for a held key's repeats. */
export function permissionKey(
  e: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey"> & {
    isComposing?: boolean;
    keyCode?: number;
    repeat?: boolean;
  },
): PermissionAnswer | null {
  if (e.altKey || e.isComposing === true || e.keyCode === 229 || e.repeat === true) return null;
  if (e.key === "Escape") return "deny";
  if (e.key !== "Enter" || e.shiftKey) return null;
  return e.metaKey || e.ctrlKey ? "always" : "once";
}

/** Where a key was pressed, as the prompt sees it: inside the prompt; in this room's composer field; with nothing
 * focused in this room; or anywhere else (another control or field, or another room). */
export type KeyWhere = "prompt" | "field" | "page" | "other";

/** Whether a key pressed there may answer the prompt. From the composer's field or from the page, only while the
 * room's field is empty, so words waiting there are never answered over; fieldEmpty is false when the room has no
 * field. */
export function keyReachesPrompt(where: KeyWhere, fieldEmpty: boolean): boolean {
  if (where === "field" || where === "page") return fieldEmpty;
  return where === "prompt";
}
