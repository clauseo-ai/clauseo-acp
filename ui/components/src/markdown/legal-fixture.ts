// The legal answer of G4's fidelity run (gate G4, clauseo-acp-private#136), and what its finished render
// must show: one home for the vitest suites here and the private real-engine harness, which imports this
// file by path (fixtures are not package exports). Copied from clauseo-acp-private at 61a2bef:
// - legal.md: measurements/g4-markdown/spike/fixtures/legal.md (30,014 characters, NFC; written by
//   fixtures/generate-legal.ts there), a contract-review memo with every feature in English, Hindi and Tamil.
// - legal-spans.json: fixtures/legal-spans.json, where each feature instance, footnote definition, literal and
//   cut point sits in legal.md.
// - fidelity-spec.json: fidelity/spec.ts's SPEC, FEATURES and RAW_KINDS and legal-content.ts's LITERAL and
//   CUTS, as data (the expectations were written before any candidate ran against them).
import LEGAL_MD from "./legal.md?raw";
import FIDELITY from "./fidelity-spec.json";
import SPANS_JSON from "./legal-spans.json";

export type Script = "en" | "hi" | "ta";

export type Expect =
  | { kind: "heading"; level: number; text: string }
  | { kind: "table"; header: string[]; rows: string[][]; align?: string[] }
  | { kind: "ordered-start"; start: number; items: string[] }
  | { kind: "nested"; types: string[]; levels: string[]; secondTopItem: string }
  | { kind: "clause-paragraphs"; texts: string[] }
  | { kind: "clause-lists"; bullet: string[]; ordered: string[] }
  | { kind: "blockquote"; outer: string[]; inner: string }
  | {
      kind: "inline";
      paragraph: string;
      bold: string;
      italic: string;
      boldItalic: string;
      struck: string;
      intraword?: { stem: string; suffix: string };
    }
  | { kind: "hr"; before: string; after: string }
  | { kind: "breaks"; lines: string[] }
  | { kind: "footnote"; sentence: string; number: string; definition: string }
  | { kind: "links"; paragraph: string; links: { href: string; text: string }[] }
  | { kind: "images"; images: { src: string; alt: string }[] }
  | {
      kind: "references";
      paragraph: string;
      refs: { component: string; ref: string; text: string }[];
    }
  | { kind: "literal"; text: string };

export type SpecEntry = { id: string; feature: string; script: string; expect: Expect };

export type Span = { id: string; feature: string; script: string; start: number; end: number };

export type Spans = {
  fixture: string;
  chars: number;
  sha256: string;
  instances: Span[];
  literals: { id: string; start: number; end: number; tokens: Record<string, number> }[];
  cuts: { id: string; instance: string; at: number }[];
};

/** The legal answer's Markdown. */
export const LEGAL: string = LEGAL_MD;
/** Where each instance sits in LEGAL. */
export const SPANS: Spans = SPANS_JSON;
/** The expected DOM of every feature instance, finished render. */
export const SPEC: readonly SpecEntry[] = FIDELITY.spec as SpecEntry[];
export const FEATURES: Readonly<Record<string, string>> = FIDELITY.features;
/**
 * Raw Markdown syntax that must not be visible in the rendered text: substrings of the answer's text, and
 * ("lead …") block elements whose own text starts with the marker. The literal paragraph L0 accounts for
 * its own `*`, `|` and `~` (SPANS.literals).
 */
export const RAW_KINDS: readonly string[] = FIDELITY.rawKinds;
