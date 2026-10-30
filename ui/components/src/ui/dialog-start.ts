// Where a dialog's focus starts, one rule for every dialog in the app. When the act can be undone it is the dark
// primary, last in the row, and focus starts on it, so Enter goes ahead. When it can't, focus starts on the safe
// choice, first in the row. The starting focus draws no ring until a key is pressed in the dialog: a dialog opened by
// a press shouldn't look as if its first button were the main one, and a key brings the ring back at once. A dialog
// that holds a form starts on its first field.

/** Marks a dialog's plane as freshly opened: data-quiet-start until a key is pressed inside it. Pass as its ref. */
export function quietStart(node: HTMLElement | null) {
  if (node === null) return;
  node.dataset["quietStart"] = "";
  const wake = () => {
    delete node.dataset["quietStart"];
    node.removeEventListener("keydown", wake, true);
  };
  node.addEventListener("keydown", wake, true);
  return () => node.removeEventListener("keydown", wake, true);
}

/** Where a dialog's plane is, once it has mounted: every lookup below stays inside it, so two rooms on one page
 * never reach into each other's dialogs. */
export type PlaneNode = () => HTMLElement | null;

/** The dialog's row of buttons, in order: the safe choice first, the act last. */
function actsIn(plane: HTMLElement | null): HTMLElement[] {
  const row = plane?.querySelector<HTMLElement>(":is([data-dialog-acts])");
  return [...(row?.querySelectorAll<HTMLElement>("button:not(:disabled), :is(a[href])") ?? [])];
}

/** Base UI's initialFocus by the rule: the act when it can be undone, the safe choice when it can't. With undoable
 * unsaid, Base UI's own first focusable stands. */
export function startOf(
  undoable: boolean | undefined,
  plane: PlaneNode,
): (() => HTMLElement | true) | undefined {
  if (undoable === undefined) return undefined;
  return () => {
    const acts = actsIn(plane());
    return (undoable ? acts[acts.length - 1] : acts[0]) ?? true;
  };
}

/** Base UI's initialFocus for a dialog that holds a form, such as New matter: its first field, so typing starts at
 * once. A field draws its own focus, so nothing here needs holding back. */
export function startAtField(plane: PlaneNode): () => HTMLElement | true {
  return () =>
    plane()?.querySelector<HTMLElement>("input:not(:disabled), textarea:not(:disabled)") ?? true;
}

/** One dialog's plane: `mount` is the popup's ref (it marks the plane quiet until a key is pressed), and `start` and
 * `field` give Base UI's initialFocus by the rules above. Make it once per dialog: useState(focusPlane). */
export function focusPlane() {
  let node: HTMLElement | null = null;
  const plane: PlaneNode = () => node;
  return {
    mount: (next: HTMLElement | null) => {
      node = next;
      const stop = quietStart(next);
      return () => {
        stop?.();
        node = null;
      };
    },
    start: (undoable: boolean | undefined) => startOf(undoable, plane),
    field: () => startAtField(plane),
  };
}
