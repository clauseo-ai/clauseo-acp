// The permission prompt's answers and its keys, apart from the DOM, so the tests can hold them.

export type PermissionAnswer = "once" | "always" | "deny";
/** Waiting for the lawyer, or answered. */
export type PermissionState = "pending" | PermissionAnswer;

/** The keys, read from a keydown: Enter, Cmd or Ctrl with Enter, Escape. Shift+Enter stays a new line. */
export function permissionKey(
  e: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey">,
): PermissionAnswer | null {
  if (e.altKey) return null;
  if (e.key === "Escape") return "deny";
  if (e.key !== "Enter" || e.shiftKey) return null;
  return e.metaKey || e.ctrlKey ? "always" : "once";
}
