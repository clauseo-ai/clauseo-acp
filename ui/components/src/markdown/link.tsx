import { useContext } from "react";
import type { KeyboardEvent, MouseEvent, ReactNode } from "react";
import { useWords } from "../lib/words.ts";
import {
  AnswerScopeContext,
  InertLinksContext,
  MarkdownHostContext,
  PaperLinksContext,
} from "./host.ts";
import { CLOBBER_PREFIX, LINK_PREFIXES, referenceOf } from "./schema.ts";
import { headingId } from "./slugs.ts";

type LinkProps = {
  href?: string | undefined;
  id?: string | undefined;
  "aria-describedby"?: string | undefined;
  /** A footnote back-reference's number (`1`, `1-2`), for its label. */
  "data-footnote-backref"?: string | undefined;
  children?: ReactNode;
  /** The link's whole text, whatever markup is inside (to-react.ts): what spoofedHost compares with. */
  text?: string | undefined;
};

/** The href a link may carry: an allowed URL, or a fragment mapped to the id it targets; else none. */
function allowed(href: string | undefined, scope: string): string | undefined {
  if (href === undefined) return undefined;
  if (href.startsWith("#")) return `#${targetId(href.slice(1), scope)}`;
  const lower = href.toLowerCase();
  return LINK_PREFIXES.some((prefix) => lower.startsWith(prefix)) ? href : undefined;
}

/**
 * The final id a fragment points at. Ids the pipeline generates already carry the answer's scope and get
 * the sanitiser's prefix; a fragment the agent wrote (`#section-2`) points at a heading, in the headings'
 * own namespace (slugs.ts), and gets the prefix too, so it never reaches a footnote's id.
 */
function targetId(fragment: string, scope: string): string {
  return fragment.startsWith(`${scope}-`)
    ? `${CLOBBER_PREFIX}${fragment}`
    : `${CLOBBER_PREFIX}${headingId(scope, fragment)}`;
}

/** A fragment's id, percent-decoded; as written when it is not valid percent-encoding (`#%E0%A4`). */
function fragmentId(fragment: string): string {
  try {
    return decodeURIComponent(fragment);
  } catch {
    return fragment;
  }
}

/** `url` with its percent-encoding undone, or as written when that is malformed. */
function decoded(url: string): string {
  try {
    return decodeURI(url);
  } catch {
    return url;
  }
}

/** Unicode's default-ignorable code points, as ranges: characters that show nothing, bidi controls, variation
 * selectors, tags (open/ask.rs's `ignorable`). */
const IGNORABLE: readonly (readonly [number, number])[] = [
  [0xad, 0xad],
  [0x34f, 0x34f],
  [0x61c, 0x61c],
  [0x115f, 0x1160],
  [0x17b4, 0x17b5],
  [0x180b, 0x180f],
  [0x200b, 0x200f],
  [0x202a, 0x202e],
  [0x2060, 0x206f],
  [0x3164, 0x3164],
  [0xfe00, 0xfe0f],
  [0xfeff, 0xfeff],
  [0xffa0, 0xffa0],
  [0xfff0, 0xfff8],
  [0x1bca0, 0x1bca3],
  [0x1d173, 0x1d17a],
  [0xe0000, 0xe0fff],
];
/** Whether a character (one code point) is default-ignorable. */
function ignorable(c: string): boolean {
  const code = c.codePointAt(0) ?? 0;
  return IGNORABLE.some(([from, to]) => code >= from && code <= to);
}
/** What reads as a full stop, read as `.` after NFKC (open/ask.rs's `reads_as_dot`), by code point. */
const DOTS = new Set(
  [
    0x3002, 0x0660, 0x06f0, 0xa60e, 0x10a50, 0x1d16d, 0x1803, 0x1809, 0xff0e, 0xff61, 0xa4f8,
    0x0701, 0x0702, 0x00b7, 0x0387, 0x2e33, 0x2219, 0x22c5, 0x06d4, 0x0589, 0x2024, 0xfe52, 0x30fb,
    0xff65,
  ].map((code) => String.fromCodePoint(code)),
);
/** An image in a link (to-react.ts's textOf), interlinear annotation marks, or any ignorable character. */
const HIDING = /[\uFFFC\uFFF9-\uFFFB]/u;
/** Characters outside ASCII that may touch a letter or digit, not a `.` (open/ask.rs's `TYPOGRAPHIC`). */
const TYPOGRAPHIC = new Set([
  "\u2018",
  "\u2019",
  "\u201C",
  "\u201D",
  "\u2013",
  "\u2014",
  "\u00A7",
  "\u20B9",
  "\u00AB",
  "\u00BB",
]);
/** A host-like token anywhere in link text: labels of letters, digits and `-`, a letter-only last label. */
const HOST_TOKEN = /(?<![\p{L}\p{N}.-])((?:[\p{L}\p{N}-]+\.)+\p{L}{2,})(?![\p{L}\p{N}-])/gu;

/** A letter or digit of any script (Rust's `char::is_alphanumeric`). */
const ALNUM = /^[\p{Alphabetic}\p{N}]$/u;

/**
 * Text as the host finder reads it (open/ask.rs's `fold`): each space outside ASCII as U+FFFD, NFKC, ignorable
 * characters dropped, what reads as a full stop as `.`, the en and em dash as `-`.
 */
function fold(text: string): string {
  const spaced = Array.from(text)
    .map((c) => (c.charCodeAt(0) > 0x7f && /^\s$/u.test(c) ? "\uFFFD" : c))
    .join("");
  return Array.from(spaced.normalize("NFKC"))
    .filter((c) => !ignorable(c))
    .map((c) => (c === "\u2013" || c === "\u2014" ? "-" : DOTS.has(c) ? "." : c))
    .join("");
}

/** A character outside ASCII next to an ASCII letter, digit or `.` (a TYPOGRAPHIC one may touch a letter or digit). */
function touchesAscii(word: string): boolean {
  const touches = (other: string, near: string) =>
    other.charCodeAt(0) > 0x7f &&
    (near === "." || (/^[A-Za-z0-9]$/.test(near) && !TYPOGRAPHIC.has(other)));
  const chars = Array.from(word);
  return chars.some((a, k) => {
    const b = chars[k + 1];
    return b !== undefined && (touches(a, b) || touches(b, a));
  });
}

/** A `.` between two letters or digits of any script. */
function dottedRun(text: string): boolean {
  const chars = Array.from(text);
  return chars.some(
    (c, k) => c === "." && ALNUM.test(chars[k - 1] ?? "") && ALNUM.test(chars[k + 1] ?? ""),
  );
}

/**
 * Whether link text could hide a host from shownHosts (open/ask.rs's `could_hide_a_host`, the same rule): an
 * ignorable character or an image; or in a word holding a character outside ASCII, such a character next to an
 * ASCII letter, digit or `.`, as written or folded, or once folded a `.` between two letters or digits.
 */
function couldHideAHost(text: string): boolean {
  if (HIDING.test(text) || Array.from(text).some(ignorable)) return true;
  return text.split(/[\t\n\f\r ]/).some((word) => {
    if (!Array.from(word).some((c) => c.charCodeAt(0) > 0x7f)) return false;
    const folded = fold(word);
    return touchesAscii(word) || touchesAscii(folded) || dottedRun(folded);
  });
}

/** Every host link text shows: width-folded, dots unified, ignorable characters dropped. */
function shownHosts(text: string): string[] {
  const plain = fold(text);
  const hosts: string[] = [];
  for (const [, token] of plain.matchAll(HOST_TOKEN)) {
    try {
      hosts.push(new URL(`https://${token}`).hostname);
    } catch {
      // Not a host after all.
    }
  }
  return hosts;
}

/** A host lower case, without a trailing `.` or a leading `www.`, as its labels from the right. */
function labels(host: string): string[] {
  const bare = host.toLowerCase().replace(/\.+$/, "");
  return (bare.startsWith("www.") && bare.length > 4 ? bare.slice(4) : bare).split(".").reverse();
}

/** One site: equal, or one a subdomain of the other, label by label (open/ask.rs's `same_site`). */
function sameSite(a: string, b: string): boolean {
  const [x, y] = [labels(a), labels(b)];
  return x.slice(0, y.length).every((label, k) => label === y[k]);
}

/**
 * The real host, shown next to link text that names another site anywhere in it (link-text spoofing:
 * `[Visit bank.example.com](https://evil.example.net)`), or that could hide one (couldHideAHost): the links the
 * host asks about before it opens them.
 */
function spoofedHost(text: string, href: string | undefined): string | undefined {
  if (!href || !/^https?:\/\//i.test(href)) return undefined;
  // An autolink's text is its destination (percent-encoded in the href): there is nothing else to show.
  if (text === href || text === decoded(href)) return undefined;
  try {
    const real = new URL(href).hostname;
    if (couldHideAHost(text)) return real;
    return shownHosts(text).every((host) => sameSite(host, real)) ? undefined : real;
  } catch {
    return undefined;
  }
}

/**
 * Every link in an answer. It renders a real <a href> for display and copy, but a click or middle click
 * never navigates: it is cancelled, and the URL goes to the host's open_external. A fragment scrolls to its
 * target. A file link that names a paper on the wall (PaperLinksContext) renders without an href, as a link the
 * keyboard reaches, and goes to the host's openPath, which opens it as the wall does; any other link is its text
 * alone, never a link that does nothing. Every link in the streaming last block renders without an href, inert. Of the sanitised attributes it keeps the id and the reference's aria-describedby; a
 * footnote back-reference gets its label in the page's language.
 */
export function Link({ href, id, children, text, ...rest }: LinkProps) {
  const words = useWords();
  const backref = rest["data-footnote-backref"];
  const host = useContext(MarkdownHostContext);
  const scope = useContext(AnswerScopeContext);
  const inert = useContext(InertLinksContext);
  const paperOf = useContext(PaperLinksContext);
  const url = inert ? undefined : allowed(href, scope);
  // a file link that names a paper on the wall: the host opens it, or says it is no longer in the folder
  const path = inert || url !== undefined || href === undefined ? undefined : paperOf(href);
  // anything else (a file elsewhere, a scheme with no action) is its text alone
  if (!inert && url === undefined && path === undefined)
    return <span data-clauseo="link-text">{children}</span>;
  const open = (event: MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    if (path !== undefined) {
      host.openPath(path);
      return;
    }
    if (!url) return;
    if (url.startsWith("#")) {
      document.getElementById(fragmentId(url.slice(1)))?.scrollIntoView();
      return;
    }
    host.openExternal(url, text);
  };
  const realHost = spoofedHost(text ?? "", url);
  return (
    <a
      href={url}
      id={id}
      aria-describedby={rest["aria-describedby"]}
      aria-label={
        backref === undefined ? undefined : words.t("Back to reference {n}", { n: backref })
      }
      data-footnote-backref={backref === undefined ? undefined : ""}
      data-clauseo="link"
      rel="noreferrer noopener"
      onClick={open}
      onAuxClick={open}
      {...(path === undefined
        ? {}
        : {
            role: "link",
            tabIndex: 0,
            onKeyDown: (event: KeyboardEvent<HTMLAnchorElement>) => {
              if (event.key !== "Enter") return;
              event.preventDefault();
              host.openPath(path);
            },
          })}
    >
      {children}
      {realHost ? <span data-clauseo="link-host"> ({realHost})</span> : null}
    </a>
  );
}

/**
 * A citation or a paper of the matter: opens in the side panel through the host, never navigates. In the
 * streaming last block it is plain, with no role and no action, until the block is finished.
 */
function Reference({
  kind,
  id,
  children,
}: {
  kind: "citation" | "paper";
  id: string;
  children?: ReactNode;
}) {
  const host = useContext(MarkdownHostContext);
  const inert = useContext(InertLinksContext);
  if (inert) {
    return (
      <span data-clauseo={kind} data-ref={id}>
        {children}
      </span>
    );
  }
  const onKeyDown = (event: KeyboardEvent<HTMLSpanElement>) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    host.openReference(kind, id);
  };
  return (
    <span
      data-clauseo={kind}
      data-ref={id}
      role="link"
      tabIndex={0}
      onClick={() => host.openReference(kind, id)}
      onKeyDown={onKeyDown}
    >
      {children}
    </span>
  );
}

/**
 * The `a` component: `clauseo-cite:<id>` and `clauseo-paper:<id>` become references when the id is valid
 * and plain text when it is not; every other link goes through Link.
 */
export function LinkOrReference(props: LinkProps) {
  const reference = referenceOf(props.href);
  if (reference === "invalid") return <>{props.children}</>;
  if (reference) {
    return (
      <Reference kind={reference.kind} id={reference.id}>
        {props.children}
      </Reference>
    );
  }
  return <Link {...props} />;
}
