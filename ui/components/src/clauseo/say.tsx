import { Fragment } from "react";
import { useDisplay } from "../lib/display.ts";
import { runs } from "../lib/runs.ts";

/** Words in the page's language, with every English run inside Hindi carrying lang="en", so shaping, hyphenation
 * and screen readers know the script. In English it's the text itself. Mixed words come back as one span holding
 * all their runs, so a flex or grid parent gets a single child and the spaces between the runs stay where the
 * source string has them, never dropped at a run's edge or widened into the parent's gap. */
export function Say({ children }: { children: string }) {
  const { lang } = useDisplay();
  const parts = runs(lang, children);
  if (parts.length === 1 && parts[0] !== undefined && !parts[0].en) return <>{children}</>;
  if (parts.length === 1 && parts[0] !== undefined) return <span lang="en">{parts[0].text}</span>;
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

/** A small round dot: the separator in a meta line. */
export function Dot() {
  return <i aria-hidden="true" className="room-dot" />;
}
