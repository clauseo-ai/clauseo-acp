// A website's tile: the first letter of its domain on the inset tone, where a list of sources leads with its icons.
//
// No favicon, on purpose. Fetching one would be a request from the webview to the site, which Clauseo never makes:
// the CSP forbids it, and Settings promises what leaves this computer. A letter needs nothing from anywhere. If the
// host ever hands the room a favicon it already holds, that may take the letter's place.
import { IconFileGeneric } from "../icons/icons.tsx";
import { langOfText } from "../lib/script.ts";
import { siteLetter } from "./site-host.ts";

/** A square tile for a website: 14 px in a row, 11 px in a folded chip. Hidden from assistive technology; the link's
 * words beside it name the site. */
export function SiteTile({ url, size }: { url: string; size: 11 | 14 }) {
  const letter = siteLetter(url);
  return (
    <span
      aria-hidden="true"
      className="site-tile"
      data-size={size}
      {...(letter === null ? {} : { lang: langOfText(letter) ?? "en" })}
    >
      {letter ?? <IconFileGeneric size={11} />}
    </span>
  );
}
