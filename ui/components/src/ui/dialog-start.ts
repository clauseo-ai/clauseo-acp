// Where a dialog's focus starts, one rule for every dialog in the app. When the act can be undone it is the dark
// primary, last in the row, and focus starts on it, so Enter goes ahead. When it can't, focus starts on the safe choice,
// first in the row. The starting focus draws no ring until a key is pressed in the dialog: a dialog opened by a press
// shouldn't look as if its first button were the main one, and a key brings the ring back at once. A dialog that holds a
// form starts on its first field.

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

/** The newest open dialog's row of buttons, in order: the safe choice first, the act last. A dialog's portal is the
 * last one opened, so the last plane in the document is the one opening. */
function actsOfNewest(): HTMLElement[] {
  const planes = document.querySelectorAll<HTMLElement>(":is([data-dialog-plane])");
  const row = planes[planes.length - 1]?.querySelector<HTMLElement>(":is([data-dialog-acts])");
  return [...(row?.querySelectorAll<HTMLElement>("button:not(:disabled), :is(a[href])") ?? [])];
}

/** Base UI's initialFocus by the rule: the act when it can be undone, the safe choice when it can't. With undoable
 * unsaid, Base UI's own first focusable stands. */
export function startOf(undoable: boolean | undefined): (() => HTMLElement | true) | undefined {
  if (undoable === undefined) return undefined;
  return () => {
    const acts = actsOfNewest();
    return (undoable ? acts[acts.length - 1] : acts[0]) ?? true;
  };
}

/** Base UI's initialFocus for a dialog that holds a form, such as New matter: its first field, so typing starts at
 * once. A field draws its own focus, so nothing here needs holding back. */
export function startAtField(): () => HTMLElement | true {
  return () => {
    const planes = document.querySelectorAll<HTMLElement>(":is([data-dialog-plane])");
    return (
      planes[planes.length - 1]?.querySelector<HTMLElement>(
        "input:not(:disabled), textarea:not(:disabled)",
      ) ?? true
    );
  };
}
