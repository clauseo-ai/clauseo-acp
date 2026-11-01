import type { Element, ElementContent } from "hast";
import { useContext } from "react";
import type { KeyboardEvent, MouseEvent, ReactNode } from "react";
import { AnswerScopeContext, InertLinksContext, MarkdownHostContext } from "./host.ts";
import { CLOBBER_PREFIX, LINK_PREFIXES, referenceOf } from "./schema.ts";

type LinkProps = {
  href?: string | undefined;
  id?: string | undefined;
  "aria-describedby"?: string | undefined;
  "aria-label"?: string | undefined;
  children?: ReactNode;
  /** The link's hast element (passNode), for its whole text whatever markup is inside. */
  node?: Element | undefined;
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
 * the sanitiser's prefix; a fragment the agent wrote (`#section-2`) gets both.
 */
function targetId(fragment: string, scope: string): string {
  return fragment.startsWith(`${scope}-`)
    ? `${CLOBBER_PREFIX}${fragment}`
    : `${CLOBBER_PREFIX}${scope}-${fragment}`;
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

/** The text of hast nodes, through any markup (`**good.com**`, `` `good.com` ``). */
function textOf(nodes: readonly ElementContent[]): string {
  let text = "";
  for (const node of nodes) {
    if (node.type === "text") text += node.value;
    else if (node.type === "element") text += textOf(node.children);
  }
  return text;
}

/** Characters that show nothing or reorder text: zero-width, bidi controls, soft hyphen, BOM. */
const INVISIBLE = /[\u00AD\u061C\u180E\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u206F\uFEFF]/g;
/** Full stops that read as `.`: ideographic, and the full-width and half-width forms NFKC may miss. */
const DOTS = /[\u3002\uFF0E\uFF61]/g;
/** A host-like token anywhere in link text: labels of letters, digits and `-`, a letter-only last label. */
const HOST_TOKEN = /(?<![\p{L}\p{N}.-])((?:[\p{L}\p{N}-]+\.)+\p{L}{2,})(?![\p{L}\p{N}-])/gu;

/** Every host link text shows: width-folded, dots unified, invisible characters dropped. */
function shownHosts(text: string): string[] {
  const plain = text.normalize("NFKC").replace(DOTS, ".").replace(INVISIBLE, "");
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

/**
 * The real host, shown next to link text that names another host anywhere in it (link-text spoofing:
 * `[Visit bank.example.com](https://evil.example.net)`).
 */
function spoofedHost(text: string, href: string | undefined): string | undefined {
  if (!href || !/^https?:\/\//i.test(href)) return undefined;
  // An autolink's text is its destination (percent-encoded in the href): there is nothing else to show.
  if (text === href || text === decoded(href)) return undefined;
  const shown = shownHosts(text);
  if (shown.length === 0) return undefined;
  try {
    const real = new URL(href).hostname;
    return shown.every((host) => host === real) ? undefined : real;
  } catch {
    return undefined;
  }
}

/**
 * Every link in an answer. It renders a real <a href> for display and copy, but a click or middle click
 * never navigates: it is cancelled, and the URL goes to the host's open_external. A fragment scrolls to its
 * target. Relative and other links render without an href, inert, and so does every link in the streaming
 * last block. Of the sanitised attributes it keeps the id and the two ARIA ones footnotes use.
 */
export function Link({ href, id, children, node, ...aria }: LinkProps) {
  const host = useContext(MarkdownHostContext);
  const scope = useContext(AnswerScopeContext);
  const inert = useContext(InertLinksContext);
  const url = inert ? undefined : allowed(href, scope);
  const open = (event: MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    if (!url) return;
    if (url.startsWith("#")) {
      document.getElementById(fragmentId(url.slice(1)))?.scrollIntoView();
      return;
    }
    host.openExternal(url);
  };
  const realHost = spoofedHost(node ? textOf(node.children) : "", url);
  return (
    <a
      href={url}
      id={id}
      aria-describedby={aria["aria-describedby"]}
      aria-label={aria["aria-label"]}
      data-clauseo="link"
      rel="noreferrer noopener"
      onClick={open}
      onAuxClick={open}
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
