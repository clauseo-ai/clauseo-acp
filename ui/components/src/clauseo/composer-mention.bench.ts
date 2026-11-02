// The `@` filter's cost per keystroke over 2,000 papers, H8's cap: run with
//   node ui/components/src/clauseo/composer-mention.bench.ts
// (Node 24 strips the types). The fixture is 2,000 paths in nested folders, a third of the names Devanagari, a sixth
// Tamil, the rest Latin. Each "keystroke" is one filterMentions call for a query typed a character at a time; the
// list is folded once, as the menu folds it when the papers arrive. The target is 8 ms a keystroke on the
// lawyer's machine; the note prints each figure ×4, as for a machine four times slower (Chrome's 4× CPU throttle).
import type { MentionItem } from "./composer.tsx";
import { filterMentions, foldMentions } from "./composer-mention-logic.ts";

const LATIN = [
  "Lease deed",
  "Notice",
  "Board minutes",
  "Written statement",
  "Exhibit",
  "Affidavit",
  "Reply",
];
const DEVA = ["क़ानूनी नोटिस", "उत्तर", "वाद पत्र", "शपथ पत्र", "आदेश"];
const TAMIL = ["கொடுப்பனவு", "அறிவிப்பு", "தீர்ப்பு"];
const FOLDERS = [
  "Pleadings",
  "Correspondence/2024",
  "Evidence/Exhibits",
  "Orders",
  "Drafts/Old versions",
];

const papers: MentionItem[] = Array.from({ length: 2000 }, (_, i) => {
  const pool = i % 6 < 2 ? DEVA : i % 6 === 2 ? TAMIL : LATIN;
  const name = `${pool[i % pool.length]} ${i}.${i % 3 === 0 ? "pdf" : "docx"}`;
  const folder = FOLDERS[i % FOLDERS.length]!;
  const path = i % 7 === 0 ? name : `${folder}/${name}`;
  return { id: `f-${i}`, label: name, detail: i % 7 === 0 ? undefined : folder, text: path };
});

// a query as typed, one code point a keystroke, as an input method that doesn't compose gives them
const typed = (q: string) => {
  const out: string[] = [];
  for (let i = 0; i < q.length; i += 1) {
    const c = q.codePointAt(i) ?? 0;
    if (c > 0xffff) i += 1;
    out.push(q.slice(0, i + 1));
  }
  return out;
};
const QUERIES = [
  ...typed("exhibit 19"),
  ...typed("notice"),
  ...typed("उत्तर"),
  ...typed("கொடு"),
  ...typed("zzz"),
  ...typed("pleadings"),
];

const t0 = performance.now();
const folded = foldMentions(papers);
const foldMs = performance.now() - t0;

// warm the JIT as a session would be warm after a few keystrokes
for (let i = 0; i < 20; i += 1) for (const q of QUERIES) filterMentions(folded, q);

const times: number[] = [];
for (let round = 0; round < 50; round += 1) {
  for (const q of QUERIES) {
    const s = performance.now();
    filterMentions(folded, q);
    times.push(performance.now() - s);
  }
}
times.sort((a, b) => a - b);
const at = (p: number) => times[Math.min(times.length - 1, Math.floor(p * times.length))]!;
const f = (ms: number) => `${ms.toFixed(3)} ms (×4: ${(ms * 4).toFixed(3)} ms)`;
console.log(`papers ${papers.length}, keystrokes ${times.length}`);
console.log(`fold once: ${f(foldMs)}`);
console.log(`per keystroke p50 ${f(at(0.5))}`);
console.log(`per keystroke p95 ${f(at(0.95))}`);
console.log(`per keystroke p99 ${f(at(0.99))}`);
console.log(`per keystroke max ${f(times[times.length - 1]!)}`);
console.log(
  `target 8 ms per keystroke: ${at(0.99) * 4 <= 8 ? "met at p99 ×4" : "NOT met at p99 ×4"}`,
);
