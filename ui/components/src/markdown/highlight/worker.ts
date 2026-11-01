// The colouring worker, and the only module that imports Shiki (worker.test.ts holds every other file to that).
// shiki/core with the JavaScript regex engine: the Oniguruma engine is WebAssembly and needs 'wasm-unsafe-eval',
// which the CSP forbids, and the JavaScript engine compiles each grammar's patterns to RegExp objects, never to code.
// Each grammar loads the first time a block in it arrives. Tokens go back by message, never HTML.
//
// Colour: the worker knows no colours. Its theme gives each TextMate scope a role (languages.ts ROLES), written as
// a stand-in colour whose number is the role's index, because a theme can carry nothing but colours; the token's
// colour is read back as that number. The page turns the role into a design-token class, so day and night come from
// the token files, and Shiki still merges neighbouring tokens of one role.
import {
  createHighlighterCore,
  type LanguageRegistration,
  type ThemeRegistration,
} from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import {
  ROLES,
  type HighlightReply,
  type HighlightRequest,
  type Language,
  type Lines,
} from "./languages.ts";

type Grammar = { default: LanguageRegistration[] };
const GRAMMARS: Record<Language, () => Promise<Grammar>> = {
  json: () => import("shiki/langs/json.mjs"),
  xml: () => import("shiki/langs/xml.mjs"),
  html: () => import("shiki/langs/html.mjs"),
  markdown: () => import("shiki/langs/markdown.mjs"),
  python: () => import("shiki/langs/python.mjs"),
  typescript: () => import("shiki/langs/typescript.mjs"),
  bash: () => import("shiki/langs/bash.mjs"),
  sql: () => import("shiki/langs/sql.mjs"),
};

/** Role index → stand-in colour: "#00000" then the index, a valid hex colour as vscode-textmate requires. */
const roleColour = (role: (typeof ROLES)[number]) =>
  `#${ROLES.indexOf(role).toString(16).padStart(6, "0")}`;

/** Which scopes take which role. TextMate picks the most specific selector, so `keyword.operator` stays plain under
 * `keyword`. */
const SCOPES: Record<Exclude<(typeof ROLES)[number], "plain">, string[]> = {
  comment: ["comment", "punctuation.definition.comment", "string.comment"],
  keyword: [
    "keyword",
    "storage",
    "storage.type",
    "storage.modifier",
    "keyword.operator.new",
    "keyword.operator.expression",
    "keyword.operator.logical.python",
    "entity.name.tag",
    "markup.heading",
    "entity.name.section",
  ],
  string: [
    "string",
    "string.regexp",
    "punctuation.definition.string",
    "markup.inline.raw",
    "markup.underline.link",
    "string.other.link",
  ],
  constant: [
    "constant",
    "constant.numeric",
    "constant.language",
    "constant.character",
    "support.constant",
    "variable.language",
    "variable.other.constant",
  ],
  function: ["entity.name.function", "support.function", "meta.function-call entity.name.function"],
  type: [
    "entity.name.type",
    "entity.name.class",
    "entity.other.inherited-class",
    "support.type",
    "support.class",
    "entity.other.attribute-name",
    "support.type.property-name",
    "meta.object-literal.key",
  ],
};

const THEME: ThemeRegistration = {
  name: "clauseo-roles",
  type: "light",
  fg: roleColour("plain"),
  bg: roleColour("plain"),
  settings: [
    { settings: { foreground: roleColour("plain"), background: roleColour("plain") } },
    { scope: ["keyword.operator", "punctuation"], settings: { foreground: roleColour("plain") } },
    ...Object.entries(SCOPES).map(([role, scope]) => ({
      scope,
      settings: { foreground: roleColour(role as keyof typeof SCOPES) },
    })),
  ],
};

let highlighter: ReturnType<typeof createHighlighterCore> | undefined;
const loading = new Map<Language, Promise<void>>();

async function ready(language: Language) {
  highlighter ??= createHighlighterCore({
    themes: [THEME],
    langs: [],
    engine: createJavaScriptRegexEngine(),
  });
  const core = await highlighter;
  let grammar = loading.get(language);
  if (!grammar) {
    grammar = GRAMMARS[language]().then((module) => core.loadLanguage(...module.default));
    loading.set(language, grammar);
  }
  await grammar;
  return core;
}

/** One block's tokens by role. Exported for the tests; the page reaches it only by message. */
export async function tokenize(code: string, language: Language): Promise<Lines> {
  const core = await ready(language);
  const lines = core.codeToTokensBase(code, {
    lang: language,
    theme: THEME.name ?? "",
    // vscode-textmate stops a line that takes longer than this and leaves the rest of it plain.
    tokenizeTimeLimit: 500,
  });
  return lines.map((line) =>
    line.map((token) => {
      const role = Number.parseInt((token.color ?? "").slice(1), 16);
      return [role > 0 && role < ROLES.length ? role : 0, token.content] as const;
    }),
  );
}

interface WorkerScope {
  addEventListener(
    type: "message",
    listener: (event: MessageEvent<HighlightRequest>) => void,
  ): void;
  postMessage(reply: HighlightReply): void;
}

// Only inside a worker: the tests import this module in Node to call tokenize().
if ("WorkerGlobalScope" in globalThis) {
  const scope = globalThis as unknown as WorkerScope;
  scope.addEventListener("message", (event) => {
    const { id, code, language } = event.data;
    tokenize(code, language).then(
      (lines) => scope.postMessage({ id, lines }),
      () => scope.postMessage({ id, lines: null }),
    );
  });
}
