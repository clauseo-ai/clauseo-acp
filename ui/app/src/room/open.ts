// H11: what the room does with the lawyer's open and show actions: a link or paper in an answer (the renderer's
// MarkdownHost) and a matter row's Show in Finder. The host draws the link's confirm itself, so a decline
// (`transport.unauthorized`) is the lawyer's own answer and says nothing. The room has no surface for a failed action
// of this kind, so any other refusal only goes to the console, the way matters.ts reports a failed command.
import type { MarkdownHost } from "@clauseo/components/markdown/host.ts";
import { toHostError, type HostTransport } from "../transport/host-transport.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { Papers } from "./host-next.ts";

/** Runs an open or show call and gives its refusal's code: null when it went through or the lawyer declined the
 * confirm; any other refusal also goes to the console. For a caller with a place to say why, such as the change note. */
export async function refusalOf(name: string, call: Promise<unknown>): Promise<string | null> {
  try {
    await call;
    return null;
  } catch (error) {
    const e = toHostError(error);
    if (e.code === "transport.unauthorized") return null;
    console.warn(`${name} failed: ${JSON.stringify(e)}`);
    return e.code;
  }
}

/** Runs an open or show call: a declined confirm is silent, any other refusal a console warning. */
export async function quietly(name: string, call: Promise<unknown>): Promise<void> {
  await refusalOf(name, call);
}

/** The papers the wall last listed, and whose they are. */
export type ListedPapers = { matter: MatterId; papers: Papers } | null;

/**
 * The answers' MarkdownHost for one matter. A paper reference's id is a `clauseo-paper:` id, which the renderer only
 * lets through as letters, digits, `_` and `-` (markdown/schema.ts), so it can't be a path with a folder or an
 * extension: it is matched to the paper whose file ref it is, or, failing that, whose matter-relative path it is
 * exactly. The papers are the ones the wall already listed, handed over with `show` as they arrive, so a new list
 * doesn't make a new host. Without them (no H8 yet, or not arrived), for another matter, or with no match, the click
 * does nothing.
 */
export class AnswerHost implements MarkdownHost {
  readonly #transport: HostTransport;
  readonly #matter: MatterId | null;
  #listed: ListedPapers = null;

  constructor(transport: HostTransport, matter: MatterId | null) {
    this.#transport = transport;
    this.#matter = matter;
  }

  /** The papers the wall listed now. */
  show(listed: ListedPapers): void {
    this.#listed = listed;
  }

  openExternal(url: string): void {
    void quietly("open_external", this.#transport.openExternal(url));
  }

  openReference(kind: "citation" | "paper", id: string): void {
    // a citation opens in the side panel: stage E's work
    if (kind !== "paper") return;
    const listed = this.#listed;
    if (this.#matter === null || listed === null || listed.matter !== this.#matter) return;
    const { papers } = listed.papers;
    const paper = papers.find((p) => p.file === id) ?? papers.find((p) => p.path === id);
    if (paper === undefined) return;
    void quietly("open_file", this.#transport.openFile(paper.file));
  }
}
