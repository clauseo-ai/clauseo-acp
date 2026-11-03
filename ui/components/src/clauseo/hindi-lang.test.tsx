// @vitest-environment happy-dom
// Hindi written by the lawyer or found in a matter, inside either interface (Hindi audit 1f): the element that holds a
// question, a pinned question, a conversation's title, a paper's name or a matter's name takes lang="hi" when its
// letters are mostly Devanagari (lib/script.ts's langOfText), so the Hindi type follows it in the English interface.
// English text in the Hindi interface gets no lang="hi". The question's computed type is checked from the sheets as
// hindi-type.test.ts reads them.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { DEFAULT_DISPLAY, showDisplay, type Lang } from "../lib/display.ts";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { QuestionLine } from "./answer.tsx";
import { PaperTile } from "./paper-tile.tsx";
import { Rail, type MatterRowProps } from "./rail.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { RoomToolbar, type RoomToolbarProps } from "./room-toolbar.tsx";
import { Thread } from "./thread.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// happy-dom has no font loading: the faces are there from the start (the thread waits on them to rest)
if (!("fonts" in document)) {
  Object.defineProperty(document, "fonts", {
    value: { ready: Promise.resolve(), status: "loaded", load: async () => [] },
  });
}

const HINDI = "धारा 138 के अंतर्गत नोटिस का उत्तर क्या होगा?";
const ENGLISH = "When did the bank reply to the notice?";

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(lang: Lang, node: ReactNode) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced", lang });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <TooltipProvider>
        <RoomScopeProvider>{node}</RoomScopeProvider>
      </TooltipProvider>,
    ),
  );
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  for (const n of document.body.querySelectorAll("[data-base-ui-portal]")) n.remove();
  document.head.innerHTML = "";
  document.documentElement.removeAttribute("lang");
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
});

const langOf = (selector: string) => {
  const node = document.querySelector(selector);
  expect(node, selector).not.toBeNull();
  return node?.getAttribute("lang") ?? null;
};

const matter = (name: string, conversation: string): MatterRowProps => ({
  name,
  note: "Recovery suit",
  hint: name,
  current: true,
  conversations: [{ id: "c1", title: conversation, agent: "Claude" }],
});

/** Each place, how to mount it with a text, and the element that holds the text. */
const PLACES: [string, (text: string) => ReactNode, string][] = [
  ["the question", (text) => <QuestionLine who="You" text={text} />, ".asked"],
  [
    "the pinned question",
    (text) => (
      <Thread question={text} who="You">
        <p>Answer.</p>
      </Thread>
    ),
    ".room-pin-text",
  ],
  [
    "a conversation's title",
    (text) => <Rail open groups={[{ when: "Today", matters: [matter("Acme v. State", text)] }]} />,
    ".room-conv-title",
  ],
  [
    "a paper tile's name",
    (text) => (
      <ol>
        <PaperTile id="f1" state="uncited" title={`${text}.pdf`} />
      </ol>
    ),
    ".tile-title",
  ],
  [
    "a matter's name in the rail",
    (text) => <Rail open groups={[{ when: "Today", matters: [matter(text, "Notice")] }]} />,
    ".room-matter-name > span",
  ],
];

test.each(PLACES)(
  "in the English interface, Hindi in %s takes lang=hi on its own element",
  async (_, node, selector) => {
    await mount("en", node(HINDI));
    expect(langOf(selector)).toBe("hi");
  },
);

test.each(PLACES)(
  "in the English interface, English in %s takes no lang=hi",
  async (_, node, selector) => {
    await mount("en", node(ENGLISH));
    expect(langOf(selector)).not.toBe("hi");
  },
);

test.each(PLACES)(
  "in the Hindi interface, English in %s takes no lang=hi",
  async (_, node, selector) => {
    await mount("hi", node(ENGLISH));
    expect(langOf(selector)).not.toBe("hi");
  },
);

test("a Hindi matter name drops the Latin cut's as-en, an English one keeps it", async () => {
  await mount("en", <Rail open groups={[{ when: "Today", matters: [matter(HINDI, "x")] }]} />);
  expect(document.querySelector(".room-matter-name > span")?.classList.contains("as-en")).toBe(
    false,
  );
  await act(async () => root?.unmount());
  host?.remove();
  await mount("en", <Rail open groups={[{ when: "Today", matters: [matter(ENGLISH, "x")] }]} />);
  const name = document.querySelector(".room-matter-name > span");
  expect(name?.classList.contains("as-en")).toBe(true);
  expect(name?.getAttribute("lang")).toBe("en");
});

/* ---------- the question's computed type ---------- */

const read = (path: string) => readFileSync(join(import.meta.dirname, path), "utf8");
/** tokens-type.css with its :lang() selectors as attribute ones (happy-dom matches no :lang()), as hindi-type.test.ts. */
const TYPE = read("../styles/tokens-type.css").replace(/:lang\((\w+)\)/g, '[lang="$1"]');
/** base.css's rule that gives a lang="hi" element its own face. */
const BASE_HI = /:where\(\[lang="hi"\]\)\s*\{[^}]*\}/.exec(read("../styles/base.css"))?.[0] ?? "";
/** What `@apply text-asked` and the body give, as Tailwind writes them: the asked size and line on the element, the
 * tokens resolved there (Chromium measured 30 px for a lang="hi" question in the English room, 28 px for English).
 * The question sets no tracking of its own; the room's title does (`tracking-title`, Chromium: -0.22 px on an English
 * name, none on one marked lang="hi"). */
const PAGE = `body { font-family: var(--font); }
  .asked { font-size: var(--t18); line-height: calc(var(--lh-asked) * var(--k)); }
  .room-title { font-size: var(--t22); letter-spacing: var(--tr-title); }
  :root { --k: 1; }`;

function sheets(lang: Lang) {
  expect(BASE_HI).not.toBe("");
  for (const css of [TYPE, BASE_HI, PAGE]) {
    const style = document.createElement("style");
    style.textContent = css;
    document.head.append(style);
  }
  document.documentElement.lang = lang;
}

const computed = (selector = ".asked") => {
  const node = document.querySelector(selector);
  if (node === null) throw new Error(selector);
  const style = getComputedStyle(node);
  const calc = /^calc\((\d+(?:\.\d+)?)px \* (\d+(?:\.\d+)?)\)$/.exec(style.lineHeight);
  return {
    face: style.fontFamily.split(",")[0]?.trim(),
    line: calc ? `${Number(calc[1]) * Number(calc[2])}px` : style.lineHeight,
    // happy-dom gives "" where no rule sets it: the browser's normal, no tracking
    tracking: style.letterSpacing === "" ? "normal" : style.letterSpacing,
  };
};

test("in the English interface, a Hindi question takes the Devanagari face and Hindi's 30 px line, untracked", async () => {
  sheets("en");
  await mount("en", <QuestionLine who="You" text={HINDI} />);
  expect(computed()).toEqual({ face: '"Anek Devanagari"', line: "30px", tracking: "normal" });
});

test("in the English interface, an English question keeps the Latin face and the 28 px line", async () => {
  sheets("en");
  await mount("en", <QuestionLine who="You" text={ENGLISH} />);
  expect(computed()).toEqual({ face: '"Anek Latin"', line: "28px", tracking: "normal" });
});

test("in the Hindi interface, an English question keeps the page's Hindi face and line", async () => {
  sheets("hi");
  await mount("hi", <QuestionLine who="You" text={ENGLISH} />);
  expect(computed()).toEqual({ face: '"Anek Devanagari"', line: "30px", tracking: "normal" });
});

const toolbar: RoomToolbarProps = {
  title: "",
  titleHint: "",
  nextDate: "",
  caseNo: "",
  court: "",
  history: [],
  canUndo: false,
  onUndo: () => {},
  railOpen: false,
  onRail: () => {},
  platform: "mac",
  plan: null,
};

test("a matter's name in Devanagari, as the room passes it (titleLang hi), heads the room untracked in the Devanagari face", async () => {
  sheets("en");
  await mount("en", <RoomToolbar {...toolbar} title="शर्मा बनाम दिल्ली राज्य" titleLang="hi" />);
  expect(langOf(".room-title")).toBe("hi");
  expect(computed(".room-title").face).toBe('"Anek Devanagari"');
  expect(computed(".room-title").tracking).toBe("0px");
});

test("an English matter name keeps the title's Latin face and its -0.01em", async () => {
  sheets("en");
  await mount("en", <RoomToolbar {...toolbar} title="Sharma v. State of Delhi" titleLang="en" />);
  expect(langOf(".room-title")).toBe("en");
  expect(computed(".room-title").face).toBe('"Anek Latin"');
  // -0.01em of the title's 22 px, as Chromium measures it
  expect(computed(".room-title").tracking).toBe("-0.22px");
});

test("in the English interface, a Devanagari paper name takes Hindi's 18 px tile line, an English one keeps 15 px", async () => {
  // wall.css's own rule for it; .facade's --title-h stands for the facade's, resolved in the page's language
  const wall = read("wall.css");
  const rule = /\.tile-title:where\(\[lang="hi"\]\)\s*\{[^}]*\}/.exec(wall)?.[0] ?? "";
  expect(rule).not.toBe("");
  sheets("en");
  const style = document.createElement("style");
  style.textContent = `.facade { --title-h: calc(var(--lh-tile) * var(--k)); }
    .tile-title { line-height: var(--title-h); } ${rule}`;
  document.head.append(style);
  await mount(
    "en",
    <ol className="facade">
      <PaperTile id="f1" state="uncited" title="क़ानूनी नोटिस.docx" />
      <PaperTile id="f2" state="uncited" title="Notice to the bank.pdf" />
    </ol>,
  );
  const [hi, en] = [...document.querySelectorAll(".tile-title")].map((n) => {
    const node = n as HTMLElement;
    const css = getComputedStyle(node);
    const calc = /^calc\((\d+)px \* (\d+)\)$/.exec(css.lineHeight);
    return calc ? `${Number(calc[1]) * Number(calc[2])}px` : css.lineHeight;
  });
  expect(hi).toBe("18px");
  expect(en).toBe("15px");
});
