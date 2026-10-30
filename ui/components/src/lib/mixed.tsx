// Words with their English runs marked. In Hindi each English run (Lease.docx, Finder, File
// Explorer, Citrix, Claude) is a span with lang="en"; in English, or with no Latin letter, the text is as it is.
import { Fragment } from "react";
import { useDisplay } from "./display.ts";
import { runs } from "./runs.ts";

/** Words in the page's language, each English run inside Hindi in a span with lang="en". Mixed words come back as one
 * span holding all their runs, so a flex or grid parent gets a single child and keeps the source string's spaces. */
export function Mixed({ children }: { children: string }) {
  const { lang } = useDisplay();
  const parts = runs(lang, children);
  const only = parts[0];
  if (parts.length === 1 && only !== undefined && !only.en) return <>{children}</>;
  if (parts.length === 1 && only !== undefined) return <span lang="en">{only.text}</span>;
  return (
    <span>
      {parts.map((r, i) =>
        r.en ? (
          <span key={i} lang="en">
            {r.text}
          </span>
        ) : (
          <Fragment key={i}>{r.text}</Fragment>
        ),
      )}
    </span>
  );
}
