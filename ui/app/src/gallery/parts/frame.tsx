// The parts gallery's frame: a group of parts as the room arranges them, and a captioned sample inside it. Captions
// are the builder's words, in English; the parts speak the page's language. Groups stack in one column and samples
// sit in a grid with explicit tracks, never CSS columns. Each sample is an inline-size container, so a part that
// measures its width measures the sample's, never the page's, and holds its layout, so a part the narrow room fixes
// to the window (the reader, the pinned question) stays inside its sample.
import type { ReactNode } from "react";

export function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section data-group className="grid gap-s4 content-start min-w-0">
      <h2 lang="en" className="font-en text-title text-primary">
        {title}
      </h2>
      <div className="grid grid-cols-1 mid:grid-cols-2 items-start gap-s6 min-w-0">{children}</div>
    </section>
  );
}

/** One part in one state. `wide` spans both tracks, for parts laid out at the room's full width. */
export function Sample({
  name,
  wide = false,
  children,
}: {
  name: string;
  wide?: boolean;
  children: ReactNode;
}) {
  return (
    <div className={`grid gap-s2 content-start min-w-0 ${wide ? "mid:col-span-2" : ""}`}>
      <h3 lang="en" className="font-en text-small text-quiet">
        {name}
      </h3>
      <div className="@container contain-layout relative min-w-0 overflow-hidden bg-surface rounded-surface shadow-e2 p-s5">
        {children}
      </div>
    </div>
  );
}
