// A part's list: Skills as two groups (built in, then added); Connectors as Clauseo Case Law's
// card, then the built-in connectors, then the lawyer's own (MCP servers). Each row is an item's
// tile, name, line and where it is on, or the word for what stands in its way; pressing it shows
// its detail beside the list, and its switch for the head's lens (L2, switches.tsx), with the
// firm's word when the firm holds it.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import type { ReactNode } from "react";
import { CASE_LAW } from "../store/connectors.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";
import { CaseLawCard, type CaseLawActs } from "./case-law.tsx";
import { ItemTile } from "./glyphs.tsx";
import { inPart, nameOf, rowLine, scopeTag, troubleTag, type Part } from "./model.ts";
import { everywhereLine } from "./scope-model.ts";
import { RowSwitch, type SwitchActs } from "./switches.tsx";

export type ListProps = {
  part: Part;
  items: readonly AddonView[] | null;
  names: ReadonlyMap<MatterId, string>;
  selected: string | null;
  onSelect: (id: string) => void;
  caseLaw: CaseLawActs;
  acts: SwitchActs;
};

function Row({ a, p }: { a: AddonView; p: ListProps }) {
  const { lang } = useDisplay();
  const trouble =
    a.status === "needs_secret" && p.caseLaw.keychain
      ? fl(lang, "Needs your Mac's permission")
      : troubleTag(lang, a.status);
  return (
    <li className="lib-row" aria-current={p.selected === a.id ? "true" : undefined}>
      <button
        type="button"
        className="lib-row-hit"
        data-testid="library-row"
        data-id={a.id}
        onClick={() => p.onSelect(a.id)}
      >
        <ItemTile addon={a} />
        <span className="lib-row-text">
          <span className="lib-row-name">
            <bdi>{nameOf(lang, a)}</bdi>
          </span>
          <span className="lib-row-line">{rowLine(lang, a)}</span>
          {p.acts.lens === "all" && a.kind === "tool" && a.source === "built_in" && (
            <span className="lib-row-note" data-testid="library-row-note">
              {everywhereLine(lang, a)}
            </span>
          )}
        </span>
        <span
          className="lib-row-tag"
          data-trouble={trouble === null ? undefined : ""}
          data-testid="library-row-tag"
        >
          {trouble ?? scopeTag(lang, a, p.names)}
          {trouble === null && a.locked && (
            <span className="lib-row-firm" data-testid="library-row-firm">
              {lb(lang, "Set by your firm")}
            </span>
          )}
        </span>
      </button>
      <RowSwitch a={a} acts={p.acts} />
    </li>
  );
}

function Group({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className="lib-group">
      <h3 className="lib-group-head">
        <span>{title}</span>
        {note !== undefined && <span className="lib-group-note">{note}</span>}
      </h3>
      {children}
    </section>
  );
}

function Rows({ items, p, empty }: { items: AddonView[]; p: ListProps; empty?: string }) {
  if (items.length === 0 && empty !== undefined)
    return <p className="lib-empty text-small text-muted">{empty}</p>;
  return (
    <ul className="lib-rows">
      {items.map((a) => (
        <Row key={a.id} a={a} p={p} />
      ))}
    </ul>
  );
}

function Loading() {
  const { lang } = useDisplay();
  return (
    <div
      className="lib-loading"
      role="status"
      aria-busy="true"
      aria-label={lb(lang, "Skills and connectors")}
    >
      {[0, 1, 2, 3].map((i) => (
        <div key={i} className="lib-ghost" aria-hidden="true">
          <span className="lib-ghost-tile" />
          <span className="lib-ghost-lines">
            <span data-n={i} />
            <span data-n={i} />
          </span>
        </div>
      ))}
    </div>
  );
}

export function PartList(p: ListProps) {
  const { lang } = useDisplay();
  if (p.items === null) return <Loading />;
  const shown = p.items.filter((a) => inPart(p.part, a));
  const builtIn = shown.filter((a) => a.source === "built_in" && a.id !== CASE_LAW);
  const added = shown.filter((a) => a.source === "added");
  if (p.part === "skills")
    return (
      <>
        <Group title={lb(lang, "Comes with Clauseo")}>
          <Rows items={builtIn} p={p} />
        </Group>
        <Group title={lb(lang, "Added by you")}>
          <Rows items={added} p={p} empty={lb(lang, "You haven't added a skill yet.")} />
        </Group>
      </>
    );
  const caseLaw = shown.find((a) => a.id === CASE_LAW && a.source === "built_in");
  return (
    <>
      {caseLaw !== undefined && (
        <CaseLawCard
          a={caseLaw}
          acts={p.caseLaw}
          names={p.names}
          selected={p.selected === caseLaw.id}
          onSelect={() => p.onSelect(caseLaw.id)}
          switches={p.acts}
        />
      )}
      {builtIn.length > 0 && (
        <Group title={lb(lang, "Comes with Clauseo")}>
          <Rows items={builtIn} p={p} />
        </Group>
      )}
      <Group title={lb(lang, "Your own connectors")} note={lb(lang, "MCP servers")}>
        <Rows
          items={added}
          p={p}
          empty={lb(lang, "You haven't added a connector of your own yet.")}
        />
      </Group>
    </>
  );
}
