// A result kept in a blob (U17, Ruled 1): read on the press, once, with one read_blob of its first 64 KB, and kept for
// the life of the thread view in a module cache keyed by its hash, the last 32 reads, so hiding and showing again sends
// nothing. A blob past 64 KB is never read, unless it holds a picture: its preview shows one (an image block's type or
// media type, or PNG, JPEG, GIF or WebP base64 as its data), or the call is agent-browser's screenshot, or it is an
// image block's own data. That one is read whole, up to 8 MB, in reads of 1 MB, so its picture can be drawn. A read
// that fails is logged, never shown, and tried again on the next press.
import type { HostTransport } from "../../../transport/host-transport.ts";
import type { BlobRef } from "../../../transport/protocol.ts";
import { quietly } from "../../open.ts";

/** The most read of a blob: 64 KB, as parse.ts's MAX_TEXT (not imported, so the cache never loads the parser). */
const MAX_READ = 65_536;
/** The most read of a blob that holds a picture: 8 MB, as an agent's longest line (the host's ACP line cap). */
export const MAX_PICTURE_READ = 8 * 1024 * 1024;
/** One read's length: the host's cap on a read_blob (MAX_BLOB_READ_BYTES). */
const CHUNK = 1024 * 1024;
/** A picture in a preview, as the compact JSON the host keeps writes it, whatever the order of the block's keys (a
 * serializer that sorts them puts `data` first): MCP's type or mimeType, Anthropic's media_type, or base64 that
 * starts as a PNG, JPEG, GIF or WebP does. */
const PICTURE =
  /"type":"image"|"mimeType":"image\/|"media_type":"image\/|"data":"(?:iVBORw0KGgo|\/9j\/|R0lGOD|UklGR)/;
/** The reads kept, at most. */
const KEPT = 32;
const reads = new Map<string, Promise<string>>();
const texts = new Map<string, string>();

/** Whether the blob is read whole for its picture: past 64 KB, within 8 MB, and a picture in its preview or the call a
 * screenshot (`screenshot`), whose text before the image can run past the preview. */
export const holdsPicture = (blob: BlobRef, screenshot = false): boolean =>
  blob.size_bytes > MAX_READ &&
  blob.size_bytes <= MAX_PICTURE_READ &&
  (screenshot || PICTURE.test(blob.preview));

/** Whether the blob is past what is read. */
export const tooLarge = (blob: BlobRef, screenshot = false): boolean =>
  blob.size_bytes > MAX_READ && !holdsPicture(blob, screenshot);

/** The blob's text when it has been read already. */
export const readAlready = (blob: BlobRef): string | undefined => texts.get(blob.hash);

/** Reads one range, the host's answer checked. */
async function readRange(transport: HostTransport, hash: string, offset: number, len: number) {
  const answer = await transport.query({ read_blob: { hash, offset, len } });
  if (answer.blob === undefined) throw new Error("read_blob answered without a blob");
  return answer.blob;
}

/** A picture's blob, whole: reads of 1 MB, each from where the last ended, until its size is read. */
async function readWhole(transport: HostTransport, blob: BlobRef): Promise<string> {
  let text = "";
  let offset = 0;
  while (offset < blob.size_bytes) {
    const range = await readRange(transport, blob.hash, offset, CHUNK);
    if (range.end <= offset) throw new Error("read_blob read nothing before the blob's end");
    text += range.text;
    offset = range.end;
  }
  return text;
}

/** Reads the blob's first 64 KB, or all of it when `whole` (a picture's, at most 8 MB), once: a read on its way or done
 * is given back. A failed read is logged and forgotten. */
export function readBlob(
  transport: HostTransport | null,
  blob: BlobRef,
  whole = holdsPicture(blob),
): Promise<string> {
  const known = reads.get(blob.hash);
  if (known !== undefined) return known;
  if (transport === null) return Promise.reject(new Error("no host to read the blob from"));
  const read = (
    whole
      ? readWhole(transport, blob)
      : readRange(transport, blob.hash, 0, MAX_READ).then((range) => range.text)
  ).then((text) => {
    texts.set(blob.hash, text);
    return text;
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
