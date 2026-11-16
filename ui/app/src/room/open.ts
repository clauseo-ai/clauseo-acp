// H11: what the room does with the lawyer's open and show actions: a link or paper in an answer
// (the renderer's MarkdownHost) and a matter row's Show in Finder. The host draws the link's
// confirm itself, so a decline (`transport.unauthorized`) is the lawyer's own answer and says
// nothing. A paper's refused open stands in the room's ActRefusals (act-refusals.ts), which the
// composer's line says; a link's other refusals only go to the console.
import type { MarkdownHost } from "@clauseo/components/markdown/host.ts";
import { toHostError, type HostTransport } from "../transport/host-transport.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { ActRefusals } from "./act-refusals.ts";
import type { Papers, PapersHost } from "./host-next.ts";

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

/** The app's own code for an open of a paper the matter's folder no longer holds (words-errors.ts's APP_ERROR_WORDS). */
export const PAPER_GONE = "paper.not_in_folder";

/**
 * The answers' MarkdownHost for one matter. A paper reference's id is a `clauseo-paper:` id, which the renderer only
 * lets through as letters, digits, `_` and `-` (markdown/schema.ts), so it can't be a path with a folder or an
 * extension: it is matched to the paper whose file ref it is, or, failing that, whose matter-relative path it is
 * exactly. The papers are the ones the wall already listed, handed over with `show` as they arrive, so a new list
 * doesn't make a new host. Without them (no H8 yet, or not arrived), for another matter, or with no match, the click
 * does nothing. A relative link (the agent's own markdown, `[Reply notes.md](Reply%20notes.md)`) opens the paper at
 * that path, as the listed papers or, failing them, a fresh list_papers has it; one in neither stands as refused
 * (PAPER_GONE), so the composer's line says the paper is no longer in the folder, never nothing.
 */
export class AnswerHost implements MarkdownHost {
  readonly #transport: HostTransport;
  readonly #matter: MatterId | null;
  readonly #refusals: ActRefusals | null;
  readonly #papers: PapersHost | null;
  #listed: ListedPapers = null;

  /** `refusals`: where a paper's refused open stands; left out, it only goes to the console. `papers`: H8's list, for a
   * relative link the listed papers don't hold. */
  constructor(
    transport: HostTransport,
    matter: MatterId | null,
    refusals: ActRefusals | null = null,
    papers: PapersHost | null = null,
  ) {
    this.#transport = transport;
    this.#matter = matter;
    this.#refusals = refusals;
    this.#papers = papers;
  }

  /** The papers the wall listed now. */
  show(listed: ListedPapers): void {
    this.#listed = listed;
  }

  openExternal(url: string, shown?: string): void {
    void quietly("open_external", this.#transport.openExternal(url, shown));
  }

  openReference(kind: "citation" | "paper", id: string): void {
    // a citation opens in the side panel: stage E's work
    if (kind !== "paper") return;
    const listed = this.#listed;
    if (this.#matter === null || listed === null || listed.matter !== this.#matter) return;
    const { papers } = listed.papers;
    const paper = papers.find((p) => p.file === id) ?? papers.find((p) => p.path === id);
    if (paper === undefined) return;
    this.#open(this.#matter, paper.file);
  }

  openPath(path: string): void {
    const matter = this.#matter;
    if (matter === null) return;
    const listed = this.#listed;
    const known = listed?.matter === matter ? fileAt(listed.papers, path) : undefined;
    if (known !== undefined) {
      this.#open(matter, known);
      return;
    }
    // the list may predate the paper: a fresh one decides
    const fresh = this.#papers?.listPapers(matter) ?? Promise.resolve(null);
    void fresh.then(
      (papers) => this.#openOrGone(matter, papers === null ? undefined : fileAt(papers, path)),
      (error: unknown) => {
        console.warn(`list_papers failed: ${JSON.stringify(error)}`);
        this.#openOrGone(matter, undefined);
      },
    );
  }

  #openOrGone(matter: MatterId, file: string | undefined): void {
    if (file !== undefined) this.#open(matter, file);
    else if (this.#refusals !== null) this.#refusals.refuse("open", matter, PAPER_GONE);
    else console.warn(`open_file: ${PAPER_GONE}`);
  }

  #open(matter: MatterId, file: string): void {
    const opened = this.#transport.openFile(file);
    void (this.#refusals === null
      ? quietly("open_file", opened)
      : this.#refusals.track("open", matter, opened));
  }
}

/** The file ref of the paper at a matter-relative path, or undefined. */
const fileAt = (papers: Papers, path: string): string | undefined =>
  papers.papers.find((p) => p.path === path)?.file;
