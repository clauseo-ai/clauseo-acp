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

function textOf(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textOf).join("");
  return "";
}

/** The real host, shown next to link text that looks like a URL for another host (link-text spoofing). */
function spoofedHost(text: string, href: string | undefined): string | undefined {
  if (!href || !/^https?:\/\//i.test(href)) return undefined;
  const shown = /^(?:https?:\/\/)?([a-z0-9.-]+\.[a-z]{2,})(?:[/:?#]|$)/i.exec(text.trim())?.[1];
  if (!shown) return undefined;
  try {
    const real = new URL(href).hostname;
    return real.toLowerCase() === shown.toLowerCase() ? undefined : real;
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
export function Link({ href, id, children, ...aria }: LinkProps) {
  const host = useContext(MarkdownHostContext);
  const scope = useContext(AnswerScopeContext);
  const inert = useContext(InertLinksContext);
  const url = inert ? undefined : allowed(href, scope);
  const open = (event: MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    if (!url) return;
    if (url.startsWith("#")) {
      document.getElementById(decodeURIComponent(url.slice(1)))?.scrollIntoView();
      return;
    }
    host.openExternal(url);
  };
  const realHost = spoofedHost(textOf(children), url);
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
