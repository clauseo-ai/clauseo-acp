// The home card offering Clauseo Case Law, the case-law connector powered by Indian Kanoon (24 Nov round, issue 8):
// where the lawyer lands with no conversation open (no matter open yet, or a new conversation before its first
// question), a card at the reading column's top says what it does, with one button to Skills and connectors, on its
// Connectors tab with Clauseo Case Law picked (library/open-library.ts's useOpenCaseLaw), whose card adds it. That
// place lists Case Law even when the host doesn't, so the button leads somewhere on the machine whose liveness check
// failed. It shows while connectors are on, the connector is not added, and Skills and connectors' list (list_addons)
// says Case Law can be set up or had no answer; never while it is not offered (store/connectors.ts's useCaseLawOffer,
// the one place a dismissal would join). It
// goes the moment a `connectors_changed` lists it added: no restart. Added but broken (a refused key, a lapsed
// sign-in) is still added, so the card stays gone and the library's and Settings' words say what's wrong. The card
// and its words come in the thread's chunk, so the room's first load carries none of them.
import { lazy, Suspense } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { addonsSourceOf, useAddons } from "../../library/addons-source.ts";
import { useOpenCaseLaw } from "../../library/open-library.ts";
import {
  CASE_LAW,
  type CaseLawListed,
  connectorsStoreOf,
  useCaseLawOffer,
  useConnector,
} from "../../store/connectors.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { ConnectorView } from "../../transport/protocol.ts";
import type { Pack } from "./case-law-card.tsx";

/** From the thread's chunk (list.tsx re-exports it), with the case-law hint's words and NoticeCard: a chunk of its
 * own would share those modules with the thread list, which vite-config.test.ts forbids. */
const CaseLawCard = lazy(() =>
  late(import("../thread/list.tsx")).then((m) => ({ default: m.CaseLawCard })),
);

/** The smallest pack, by its credits: the gross price the host gives (`price_paise`) and that pack's credits; null
 * while the connector isn't listed or the host gives no price. */
function packOf(c: ConnectorView | undefined): Pack | null {
  const paise = c?.price_paise;
  const smallest = c?.packs.reduce<ConnectorView["packs"][number] | null>(
    (min, p) => (min === null || p.credits < min.credits ? p : min),
    null,
  );
  return paise === undefined || smallest == null ? null : { paise, credits: smallest.credits };
}

/** Case Law's status in the list, as the card reads it; null until the list has answered. */
function useListed(transport: HostTransport): CaseLawListed {
  const items = useAddons(addonsSourceOf(transport)).items;
  if (items === null) return null;
  const status = items.find((a) => a.id === CASE_LAW && a.source === "built_in")?.status;
  return status === "needs_setup" || status === "unreachable" ? status : "other";
}

/** The card while it shows; nothing otherwise, or outside the room, where there is no Skills and connectors. */
export function CaseLawOffer({ transport }: { transport: HostTransport }) {
  const openCaseLaw = useOpenCaseLaw();
  const store = connectorsStoreOf(transport);
  const shows = useCaseLawOffer(store, useListed(transport));
  const pack = packOf(useConnector(store, CASE_LAW));
  if (!shows || openCaseLaw === null) return null;
  return (
    <Suspense fallback={null}>
      <CaseLawCard pack={pack} onAdd={openCaseLaw} />
    </Suspense>
  );
}
