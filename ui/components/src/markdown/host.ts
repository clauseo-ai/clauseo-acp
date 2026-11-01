// What the renderer asks of the host, and the answer's id scope. The app provides MarkdownHost later
// (open_external, open_reference); until then every action is a no-op. The webview never navigates:
// every link click is cancelled and handed here instead.
import { createContext } from "react";

export type MarkdownHost = {
  /** Open an http(s) or mailto URL outside the app. */
  openExternal(url: string): void;
  /** Open a citation or a paper of the matter, by its validated id. */
  openReference(kind: "citation" | "paper", id: string): void;
};

const NO_HOST: MarkdownHost = {
  openExternal() {},
  openReference() {},
};

export const MarkdownHostContext = createContext<MarkdownHost>(NO_HOST);

/** The answer's own part of every generated id, so many answers on one page never share an id. */
export const AnswerScopeContext = createContext("");

/** A React id (`useId`) as an id-safe scope: letters, digits, `_` and `-` only. */
export function scopeOf(reactId: string): string {
  return `md${reactId.replace(/[^A-Za-z0-9_-]/g, "")}`;
}
