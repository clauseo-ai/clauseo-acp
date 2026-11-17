// Create's press while a synced folder's notice waits (use-scan.ts awaitingGotIt): focus goes to the notice's Got it
// (place-notice.tsx), which then draws the app's focus ring. Part of New matter's chunk (folder-check.tsx).
/** Create was pressed while the notice waits: focus goes to Got it, which draws the app's focus ring. After a pointer's
 * press the browser often draws no :focus-visible ring on a script's focus, so the ring is marked here too
 * (data-focus-moved, base.css), until focus leaves. */
export function focusGotIt(el: HTMLElement | null | undefined) {
  if (el == null) return;
  el.dataset["focusMoved"] = "";
  el.addEventListener("blur", () => delete el.dataset["focusMoved"], { once: true });
  el.focus();
}
