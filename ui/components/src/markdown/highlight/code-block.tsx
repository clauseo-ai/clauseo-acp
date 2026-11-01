// A code block that colours itself: plain `<pre><code>` until the worker's tokens arrive, then one span per token,
// coloured by its role's design-token class. React elements only, never an HTML string.
import type { HighlightClient } from "./client.ts";
import type { Lines } from "./languages.ts";
import { useHighlight } from "./use-highlight.ts";

/** Each role's class, by index in ROLES: the code colour tokens in styles/tokens-colour.css, day and night. */
const ROLE_CLASS: readonly string[] = [
  "",
  "text-code-comment",
  "text-code-keyword",
  "text-code-string",
  "text-code-constant",
  "text-code-function",
  "text-code-type",
];

export interface HighlightedCodeProps {
  /** The block's code, without the fence and without its last newline. */
  readonly code: string;
  /** The fence's language name, as written; any language outside the worker's eight stays plain. */
  readonly lang: string;
  /** True once the fence has closed. Until then the block is plain and nothing is asked of the worker. */
  readonly finished: boolean;
  /** Classes for the `<pre>`. */
  readonly className?: string;
  /** For the tests; the page's one client otherwise. */
  readonly client?: HighlightClient;
}

export function HighlightedCode({ code, lang, finished, className, client }: HighlightedCodeProps) {
  const lines = useHighlight(code, lang, finished, client);
  return (
    <pre
      className={className}
      data-lang={lang || undefined}
      data-highlighted={lines ? "" : undefined}
    >
      <code>{lines ? <TokenLines lines={lines} /> : code}</code>
    </pre>
  );
}

/** The tokens as spans, one line per span with its newline, so copying the block gives back its code. */
export function TokenLines({ lines }: { readonly lines: Lines }) {
  return lines.map((line, index) => (
    <span key={index}>
      {line.map(([role, text], at) => {
        const roleClass = ROLE_CLASS[role] ?? "";
        return roleClass ? (
          <span key={at} className={roleClass}>
            {text}
          </span>
        ) : (
          text
        );
      })}
      {index < lines.length - 1 ? "\n" : null}
    </span>
  ));
}
