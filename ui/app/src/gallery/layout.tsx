// The gallery's frame: a titled section on the raised surface, and a captioned state inside it. Captions are the
// builder's words, in English; the specimens speak the page's language.
import type { ReactNode } from "react";

export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="grid gap-s3 content-start min-w-0">
      <h2 lang="en" className="font-en text-title text-primary">
        {title}
      </h2>
      <div className="flex flex-wrap items-end gap-x-s6 gap-y-s4 min-w-0 bg-surface rounded-surface shadow-e2 p-s5">
        {children}
      </div>
    </section>
  );
}

export function State({ name, children }: { name: string; children: ReactNode }) {
  return (
    <div className="grid gap-s2 justify-items-start min-w-0 max-w-full">
      <span lang="en" className="font-en text-small text-quiet">
        {name}
      </span>
      {children}
    </div>
  );
}
