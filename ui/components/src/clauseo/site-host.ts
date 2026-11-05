// A website's host and its first letter, read from the address as written, for its tile (site-tile.tsx).

/** The address's host, its "www." dropped, or null for an address with no host (about:blank, file:, data:). A host
 * written in Devanagari stays in Devanagari: the address is read as written, never through URL, which would turn it
 * into punycode. */
export function siteHost(url: string): string | null {
  const text = url.trim();
  const scheme = /^([a-z][a-z\d+.-]*):/i.exec(text);
  let rest: string;
  if (scheme === null) {
    // a bare domain, as an agent may write one: example.com/page
    rest = text;
  } else if (text.slice(scheme[0].length).startsWith("//")) {
    rest = text.slice(scheme[0].length + 2);
  } else {
    return null;
  }
  const authority = rest.split(/[/?#\\]/, 1)[0] ?? "";
  const host = (authority.split("@").pop() ?? "")
    .replace(/:\d*$/, "")
    .replace(/\.$/, "")
    .replace(/^www\./i, "");
  if (host === "" || (scheme === null && !host.includes("."))) return null;
  return host;
}

/** The host's first letter, whole: a Devanagari letter keeps its vowel sign. Uppercase where the script has case. */
export function siteLetter(url: string): string | null {
  const host = siteHost(url);
  if (host === null) return null;
  const first = new Intl.Segmenter(undefined, { granularity: "grapheme" })
    .segment(host)
    [Symbol.iterator]()
    .next();
  return first.done ? null : first.value.segment.toUpperCase();
}
