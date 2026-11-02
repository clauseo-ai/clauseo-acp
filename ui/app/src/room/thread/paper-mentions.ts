// The composer's `@` papers and what a send carries for them. The papers are H8's `list_papers` for the open matter, read
// through the transport's papers part as the wall reads them (host-next.ts), only once `@` first opens the menu, and
// kept per transport and matter: a later `@` within 30 s takes the kept list, a later one asks the host again, as the
// host sends no event when a matter's papers change. A failed read is not kept, so the next `@` asks again. Each paper
// is a mention: its FileRefId, its file name, its folder, and its path in the matter folder for the token's words.
// A send's prompt is the text, then one `File` block for each paper its tokens mention, in the order they first stand
// in the text, each once (promptContent).
import type { MentionItem, MentionList, Mentions } from "@clauseo/components/clauseo/composer.tsx";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FileRefId, MatterId, PromptBlock } from "../../transport/protocol.ts";
import { nextHostOf, type Papers } from "../host-next.ts";

/** How long a matter's papers, once read, answer a new `@` without asking the host again. */
export const PAPERS_FRESH_MS = 30_000;

/** A matter's papers as mentions, in list_papers' order (by path), and whether the list stops short (H8's cap). */
export function papersToMentions(papers: Papers): MentionList {
  return {
    items: papers.papers.map((p): MentionItem => {
      const cut = p.path.lastIndexOf("/");
      return {
        id: p.file,
        label: cut === -1 ? p.path : p.path.slice(cut + 1),
        detail: cut === -1 ? undefined : p.path.slice(0, cut),
        text: p.path,
      };
    }),
    partial: papers.truncated,
  };
}

type Kept = { at: number; answer: Promise<MentionList> };
const kept = new WeakMap<HostTransport, Map<MatterId, Kept>>();

/** The composer's `mentions` for a matter: undefined with no matter open, or when the transport's host has no
 * list_papers, so `@` stays a character. One object per transport and matter is enough (Work memoises it); the list
 * itself is kept across them. */
export function paperMentions(
  transport: HostTransport,
  matter: MatterId | null,
  clock: () => number = Date.now,
): Mentions | undefined {
  const host = nextHostOf(transport).papers;
  if (host === null || matter === null) return undefined;
  return {
    load: () => {
      let byMatter = kept.get(transport);
      if (byMatter === undefined) {
        byMatter = new Map();
        kept.set(transport, byMatter);
      }
      const now = clock();
      const was = byMatter.get(matter);
      if (was !== undefined && now - was.at < PAPERS_FRESH_MS) return was.answer;
      const answer = host.listPapers(matter).then(papersToMentions);
      const entry = { at: now, answer };
      byMatter.set(matter, entry);
      const map = byMatter;
      answer.catch(() => {
        if (map.get(matter) === entry) map.delete(matter);
      });
      return answer;
    },
  };
}

/** A send's prompt: the text, then one `File` block per paper, in the order given (the field's tokens first, then any
 * paper pinned to the question as a chip), each FileRefId once. */
export function promptContent(text: string, files: readonly FileRefId[] = []): PromptBlock[] {
  const blocks: PromptBlock[] = [{ text: { text } }];
  const seen = new Set<FileRefId>();
  for (const file of files) {
    if (seen.has(file)) continue;
    seen.add(file);
    blocks.push({ file: { file } });
  }
  return blocks;
}
