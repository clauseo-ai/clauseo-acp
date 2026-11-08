// The room's start-up marks, so a slow start can be split into its phases from host.log alone: four
// `performance.mark`s (the script's start, the root's render called, the welcome's first commit, its primary button
// mounted), sent once, after the button mounts, as one background call (`page_marks`, transport's `pageMarks`). Each
// goes as how long ago it was, read as the call leaves the page; the desktop puts it on its own clock (ms since `run`).
// Nothing waits on them: the call goes after the button is up, on the background lane, and a failure only loses the
// line. A start with no welcome (a matter already there) sends nothing.
//
// main.tsx imports this module first, so its body runs before any other module of the first load: the script mark.
import type { PageMarks } from "./transport/bindings.ts";

/** The marks' names on the page's performance timeline. */
export const MARKS = {
  script: "clauseo:script",
  render: "clauseo:render",
  welcome: "clauseo:welcome",
  button: "clauseo:button",
} as const;

/** What `page_marks` carries (desktop's startup.rs is its home): for each mark, the ms from it to the call leaving the
 * page; null when it was not taken. `navigation` is the page's time origin. */
export type { PageMarks };

/** Sends the marks once; `read` gives them as the call leaves. The Tauri transport's `pageMarks`. */
export type SendMarks = (read: () => PageMarks) => Promise<void>;

const timeline = (): Performance | null =>
  typeof performance === "object" && typeof performance.mark === "function" ? performance : null;

const mark = (name: string) => void timeline()?.mark(name);

mark(MARKS.script);

/** The page's root render, called. */
export const markRender = (): void => mark(MARKS.render);

let welcomed = false;

/** The welcome's first commit; later ones change nothing. */
export function markWelcome(): void {
  if (welcomed) return;
  welcomed = true;
  mark(MARKS.welcome);
}

/** ms from `at` (performance.now's clock) to `now`, whole and within a u32's half. */
const ago = (now: number, at: number) => Math.max(0, Math.min(Math.round(now - at), 2 ** 31));

/** The marks taken so far, each as how long ago it was. */
export function readMarks(): PageMarks {
  const now = performance.now();
  const at = (name: string) => {
    const entry = performance.getEntriesByName(name, "mark")[0];
    return entry === undefined ? null : ago(now, entry.startTime);
  };
  return {
    navigation: ago(now, 0),
    script: at(MARKS.script),
    render: at(MARKS.render),
    welcome: at(MARKS.welcome),
    button: at(MARKS.button),
  };
}

let sent = false;

/** The welcome's primary button, mounted (its ref): marks it, then, after the commit, sends every mark once through
 * `send` (none where the transport has no `pageMarks`). Later calls, and a null element (the ref's detach), change
 * nothing. */
export function markButton(element: Element | null, send: SendMarks | undefined): void {
  if (element === null || sent) return;
  sent = true;
  mark(MARKS.button);
  if (send === undefined) return;
  // after the commit, so the welcome's own mark (a layout effect of the same commit) is in
  queueMicrotask(() => void send(readMarks).catch(() => {}));
}

/** Forgets what was marked and sent since the script's start, for tests. */
export function resetMarks(): void {
  welcomed = false;
  sent = false;
  for (const name of [MARKS.render, MARKS.welcome, MARKS.button]) timeline()?.clearMarks(name);
}
