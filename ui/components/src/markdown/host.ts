// What the renderer asks of the host, and the answer's id scope. The app provides MarkdownHost later
// (open_external, open_reference); until then every action is a no-op. The webview never navigates:
// every link click is cancelled and handed here instead.
import { createContext } from "react";

export type MarkdownHost = {
  /** Open an http(s) or mailto URL outside the app; `shown` is the link's visible text, which the host compares with
   * where the link goes before it opens it unasked. */
  openExternal(url: string, shown?: string): void;
  /** Open a citation or a paper of the matter, by its validated id. */
  openReference(kind: "citation" | "paper", id: string): void;
  /** Open the paper on the wall a file link names (PaperLinksContext), by its matter-relative path; the host says so
   * when the matter's folder no longer holds it. */
  openPath(path: string): void;
};

const NO_HOST: MarkdownHost = {
  openExternal() {},
  openReference() {},
  openPath() {},
};

export const MarkdownHostContext = createContext<MarkdownHost>(NO_HOST);

/**
 * Which paper on the wall a file link names (`/Users/…/Reply%20to%20notice.docx`, `file://…`, `sandbox:/mnt/data/…`,
 * `Reply.docx`): that paper's matter-relative path, which the link hands to `openPath`, or undefined. A file link it
 * names no paper for shows as plain text, never as a link that does nothing. The room gives one that reads the wall;
 * with none (every other surface) no file link matches.
 */
export type PaperLinks = (href: string) => string | undefined;

export const PaperLinksContext = createContext<PaperLinks>(() => undefined);

/**
 * True inside the streaming last block: its links render inert (no href, no action) until it is finished,
 * since a URL or reference id may still be arriving.
 */
export const InertLinksContext = createContext(false);

/**
 * The words of a code block's Copy (D-026), as the host gives them: Copy, or Copied for a moment after a press. With
 * none (the default, and every surface but the thread's answers), a code block is its `pre` alone.
 */
export const CodeCopyContext = createContext<((copied: boolean) => string) | null>(null);

/** The answer's own part of every generated id, so many answers on one page never share an id. */
export const AnswerScopeContext = createContext("");

/** A React id (`useId`) as an id-safe scope: letters, digits, `_` and `-` only. */
export function scopeOf(reactId: string): string {
  return `md${reactId.replace(/[^A-Za-z0-9_-]/g, "")}`;
}
