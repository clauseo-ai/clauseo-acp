// What both sides of the colouring worker agree on, with nothing from Shiki: the languages it colours, the names an
// answer may give them, the roles a token can take, and the messages between the page and the worker.

/** The grammars the worker carries: G4's eight, the languages agents put in the answers lawyers read. */
export const LANGUAGES = [
  "json",
  "xml",
  "html",
  "markdown",
  "python",
  "typescript",
  "bash",
  "sql",
] as const;
export type Language = (typeof LANGUAGES)[number];

/** The other names a fence's info string uses for them. */
const ALIASES: Readonly<Record<string, Language>> = {
  ts: "typescript",
  tsx: "typescript",
  js: "typescript",
  jsx: "typescript",
  javascript: "typescript",
  mjs: "typescript",
  cjs: "typescript",
  py: "python",
  python3: "python",
  sh: "bash",
  shell: "bash",
  shellscript: "bash",
  zsh: "bash",
  console: "bash",
  md: "markdown",
  htm: "html",
  xhtml: "html",
  svg: "xml",
  jsonc: "json",
  json5: "json",
  postgresql: "sql",
  mysql: "sql",
  sqlite: "sql",
};

/** The grammar for a fence's language name, or undefined: a block in any other language stays plain, and asks the
 * worker nothing. */
export function languageOf(name: string): Language | undefined {
  const lower = name.trim().toLowerCase();
  if ((LANGUAGES as readonly string[]).includes(lower)) return lower as Language;
  return ALIASES[lower];
}

/** A token's role, which picks its colour token. 0 is plain code, in the block's own text colour. The worker's
 * theme gives each TextMate scope one of these; the page maps each to a class (code-block.tsx). */
export const ROLES = [
  "plain",
  "comment",
  "keyword",
  "string",
  "constant",
  "function",
  "type",
] as const;
export type Role = number;

/** One token: its role (an index into ROLES) and its text. */
export type Token = readonly [role: Role, text: string];
/** A block's tokens, line by line, without the newlines. */
export type Lines = readonly (readonly Token[])[];

/** The page asks: colour this code in this language. */
export interface HighlightRequest {
  readonly id: number;
  readonly language: Language;
  readonly code: string;
}
/** The worker answers with tokens, or with null when it could not colour the block. Never HTML. */
export interface HighlightReply {
  readonly id: number;
  readonly lines: Lines | null;
}
