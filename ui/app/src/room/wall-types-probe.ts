// ?papers=types: the probe's wall with a file of every type its tiles colour (PDF, Word, Excel, CSV, PowerPoint) and
// the neutral ones (an image, a Markdown file, a plain text file), with three files only in OneDrive and a folder not
// opened among them, dimmed, for the file-type colours' screenshots in day and night. Only the probe build imports this
// file. Not shipped.
import type { Papers } from "./host-next.ts";

/** The papers' names, under papers/, in the order the host lists them. */
const TYPES = [
  "Bank statement, March 2001.xlsx",
  "Cheque register.csv",
  "Hearing outline.pptx",
  "List of dates.md",
  "Notice to the bank.pdf",
  "Reply to the notice.docx",
  "Scan of the cheque.jpg",
  "Transcript of the call.txt",
] as const;

/** ?papers=types' names and the listing beside them, or none for any other value. */
export function wallTypesProbe(
  value: string | null,
): { names: readonly string[]; listing: Partial<Omit<Papers, "papers">> } | null {
  if (value !== "types") return null;
  return {
    names: TYPES,
    listing: {
      place: { synced: { service: "one_drive" } },
      skipped: [
        { path: "papers/Ledger, 2000.xlsx", reason: "cloud_only" },
        { path: "papers/Lease deed.pdf", reason: "cloud_only" },
        { path: "papers/Old reply.docx", reason: "cloud_only" },
      ],
      unread: ["papers/Annexures"],
    },
  };
}
