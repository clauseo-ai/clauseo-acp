// A result kept in a blob (U17, Ruled 1): read on the press, once, with one read_blob of its first 64 KB, and kept for
// the life of the thread view in a module cache keyed by its hash, the last 32 reads, so hiding and showing again sends
// nothing. A blob past 64 KB is never read; a read that fails is logged, never shown, and tried again on the next press.
import type { HostTransport } from "../../../transport/host-transport.ts";
import type { BlobRef } from "../../../transport/protocol.ts";
import { quietly } from "../../open.ts";

/** The most read of a blob: 64 KB, as parse.ts's MAX_TEXT (not imported, so the cache never loads the parser). */
const MAX_READ = 65_536;
/** The reads kept, at most. */
const KEPT = 32;
const reads = new Map<string, Promise<string>>();
const texts = new Map<string, string>();

/** Whether the blob is past what is read. */
export const tooLarge = (blob: BlobRef): boolean => blob.size_bytes > MAX_READ;

/** The blob's text when it has been read already. */
export const readAlready = (blob: BlobRef): string | undefined => texts.get(blob.hash);

/** Reads the blob's first 64 KB, once: a read on its way or done is given back. A failed read is logged and forgotten. */
export function readBlob(transport: HostTransport | null, blob: BlobRef): Promise<string> {
  const known = reads.get(blob.hash);
  if (known !== undefined) return known;
  if (transport === null) return Promise.reject(new Error("no host to read the blob from"));
  const read = transport
    .query({ read_blob: { hash: blob.hash, offset: 0, len: MAX_READ } })
    .then((answer) => {
      if (answer.blob === undefined) throw new Error("read_blob answered without a blob");
      texts.set(blob.hash, answer.blob.text);
      return answer.blob.text;
    });
  reads.set(blob.hash, read);
  for (const old of reads.keys()) {
    if (reads.size <= KEPT) break;
    reads.delete(old);
    texts.delete(old);
  }
  read.catch((error: unknown) => {
    if (reads.get(blob.hash) === read) reads.delete(blob.hash);
    void quietly("read_blob", Promise.reject(error));
  });
  return read;
}

/** Empties the cache, as a new thread view has it. For tests only. */
export function forgetBlobsForTests(): void {
  reads.clear();
  texts.clear();
}
