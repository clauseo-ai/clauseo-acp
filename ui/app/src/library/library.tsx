// Skills and connectors, the rail's place in the room's centre (24 Nov round, library L1,
// read-only): the two parts as tabs, the part's list, and the panel beside it, which explains the
// part until an item is picked and then shows that item. The list comes through addons-source.ts
// alone, read again each time the place opens. Switches, scope, adding and removing come later (L2
// to L4); Clauseo Case Law's Add opens today's flow, Settings at Connectors.
import "./library.css";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import { Tabs, TabsList, TabsPanel } from "@clauseo/components/ui/tabs.tsx";
import { useEffect, useMemo, useRef, useState } from "react";
import { quietly } from "../room/open.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { AgentKind, MatterId } from "../transport/protocol.ts";
import { useAddons, type AddonsSource } from "./addons-source.ts";
import { useAssistant } from "./assistant.ts";
import type { CaseLawActs } from "./case-law.tsx";
import { Explainer, ItemDetail } from "./detail.tsx";
import { PartList } from "./list.tsx";
import { inPart, type Part } from "./model.ts";
import type { LibraryStart } from "./open-library.ts";

export type { LibraryStart } from "./open-library.ts";

const PARTS: readonly Part[] = ["skills", "connectors"];

export function Library({
  transport,
  source,
  names,
  plan,
  start,
}: {
  transport: HostTransport;
  source: AddonsSource;
  /** The matters' names by id, for "Only Sharma v. State of Delhi". */
  names: ReadonlyMap<MatterId, string>;
  /** The kind of plan the room's new conversations run: names the assistant until a sign-in state
   * is heard. */
  plan: AgentKind | undefined;
  /** Where the place opens: a part, and an item picked (the home card's "More about it"). */
  start?: LibraryStart | undefined;
}) {
  const { lang } = useDisplay();
  const [part, setPart] = useState<Part>(start?.part ?? "skills");
  const [selected, setSelected] = useState<string | null>(start?.selected ?? null);
  const { items, caseLaw: facts } = useAddons(source);
  useEffect(() => source.read(), [source]);
  const assistant = useAssistant(transport, plan);
  const caseLaw = useMemo<CaseLawActs>(() => {
    const payUrl = facts.out?.payUrl ?? null;
    return {
      facts,
      openConnectors: source.openConnectors,
      recheck: source.recheck,
      addCredits:
        payUrl === null
          ? source.openConnectors
          : () => void quietly("open_pay_link", transport.openExternal(payUrl, undefined, true)),
    };
  }, [facts, source, transport]);
  // the rail's row that opened the place may fold away under the focus: the chosen tab takes it
  const place = useRef<HTMLDivElement>(null);
  useEffect(() => {
    place.current?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
  }, []);
  const picked = items?.find((a) => a.id === selected && inPart(part, a)) ?? null;
  return (
    <div className="lib-place" data-testid="library" ref={place}>
      <Tabs
        value={part}
        onValueChange={(v) => {
          setPart(v as Part);
          setSelected(null);
        }}
        className="lib-tabs"
      >
        <TabsList
          label={lb(lang, "Skills and connectors")}
          tabs={[
            { value: "skills", label: lb(lang, "Skills") },
            { value: "connectors", label: lb(lang, "Connectors") },
          ]}
        />
        {PARTS.map((p) => (
          <TabsPanel key={p} value={p} className="lib-split">
            {p === part && (
              <>
                <div className="lib-list">
                  <PartList
                    part={part}
                    items={items}
                    names={names}
                    selected={picked?.id ?? null}
                    onSelect={(id) => setSelected(selected === id ? null : id)}
                    caseLaw={caseLaw}
                  />
                </div>
                <aside className="lib-side" data-testid="library-side">
                  {picked === null ? (
                    <Explainer part={part} who={assistant.who} />
                  ) : (
                    <ItemDetail a={picked} names={names} assistant={assistant} caseLaw={caseLaw} />
                  )}
                </aside>
              </>
            )}
          </TabsPanel>
        ))}
      </Tabs>
    </div>
  );
}
