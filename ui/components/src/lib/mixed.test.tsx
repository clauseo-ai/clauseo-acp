// @vitest-environment happy-dom
// Mixed words in a flex box. Inside Hindi, every English run is its own span with lang="en"; a flex or grid parent
// makes each of its children a box of its own and drops the spaces at a box's edges, so "FIR और रिमांड" would read
// "FIRऔर रिमांड" if its runs reached the parent one by one. <Mixed> hands the parent one span holding all the runs.
// This renders every Hindi string in the words files inside an inline-flex box, and checks the text a reader sees,
// each flex item with its edges trimmed as layout trims them, keeps the source's spaces.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, beforeAll, expect, test } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "./display.ts";
import { Mixed } from "./mixed.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const DEVA = /[\u0900-\u097F]/;
const LATIN = /[A-Za-z]/;

/** Every string literal holding Devanagari in the words files, with each slot filled by an English word, so the
 * filled string has English runs where the screen has them. */
function hindiStrings(): string[] {
  const files: Record<string, string> = {
    ...import.meta.glob<string>(["./words*.ts", "!./*.test.ts"], {
      query: "?raw",
      import: "default",
      eager: true,
    }),
  };
  const out = new Set<string>();
  for (const src of Object.values(files)) {
    for (const m of src.matchAll(
      /"((?:[^"\\\n]|\\.)*)"|'((?:[^'\\\n]|\\.)*)'|`((?:[^`\\$]|\\.)*)`/g,
    )) {
      const raw = m[1] ?? m[2] ?? m[3] ?? "";
      if (!DEVA.test(raw)) continue;
      const s = raw
        .replace(/\\u([0-9a-fA-F]{4})/g, (_, h: string) => String.fromCharCode(parseInt(h, 16)))
        .replace(/\\(.)/g, "$1");
      out.add(s.replace(/\{\w+\}/g, "Word"));
    }
  }
  return [...out];
}

/** The text a reader sees in a flex box: each child is a flex item, whose collapsible spaces at its edges go. */
function seenInFlex(box: Element): string {
  return [...box.childNodes].map((n) => (n.textContent ?? "").replace(/\s+/g, " ").trim()).join("");
}
const collapsed = (s: string) => s.replace(/\s+/g, " ").trim();

const strings = hindiStrings();
const mixed = strings.filter((s) => LATIN.test(s));
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

beforeAll(() => {
  showDisplay({ ...DEFAULT_DISPLAY, lang: "hi" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterAll(() => {
  act(() => root.unmount());
  host.remove();
  showDisplay(DEFAULT_DISPLAY);
});

test("the words files hold the Hindi this test reads", () => {
  expect(strings.length).toBeGreaterThan(80);
  expect(mixed.length).toBeGreaterThan(10);
  expect(mixed).toContain("File Explorer में दिखाएँ");
});

for (const [name, Part] of [["Mixed", Mixed]] as const) {
  test(`${name}: every Hindi string keeps its spaces inside an inline-flex box`, async () => {
    await act(async () =>
      root.render(
        <>
          {strings.map((s, i) => (
            <span key={i} className="flexbox" style={{ display: "inline-flex", gap: "8px" }}>
              <Part>{s}</Part>
            </span>
          ))}
        </>,
      ),
    );
    const boxes = [...host.querySelectorAll(".flexbox")];
    expect(boxes).toHaveLength(strings.length);
    const broken = boxes.flatMap((b, i) => {
      const s = strings[i] ?? "";
      return b.childNodes.length === 1 && seenInFlex(b) === collapsed(s) ? [] : [s];
    });
    expect(broken).toEqual([]);
  });
}

test("the English runs inside Hindi still carry lang=en, inside the one span", async () => {
  await act(async () =>
    root.render(
      <span className="flexbox" style={{ display: "inline-flex" }}>
        <Mixed>{"शाम 4:44 बजे से Word में खुला"}</Mixed>
      </span>,
    ),
  );
  const box = host.querySelector(".flexbox");
  expect(box?.children).toHaveLength(1);
  expect(box?.querySelector('[lang="en"]')?.textContent).toBe("Word");
  expect(box?.textContent).toBe("शाम 4:44 बजे से Word में खुला");
});

test("words with one run come back as they are, a wholly English run with lang=en", async () => {
  await act(async () =>
    root.render(
      <>
        <span className="one">
          <Mixed>{"केवल हिंदी"}</Mixed>
        </span>
        <span className="two">
          <Mixed>{"Lease.docx"}</Mixed>
        </span>
      </>,
    ),
  );
  expect(host.querySelector(".one")?.innerHTML).toBe("केवल हिंदी");
  expect(host.querySelector(".two")?.innerHTML).toBe('<span lang="en">Lease.docx</span>');
});
