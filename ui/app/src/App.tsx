import type { HostTransport } from "./transport/host-transport.ts";

/** The whole app, for now: one empty root element. The transport is how it will reach the host. */
export function App(_props: { transport: HostTransport }) {
  // scripts/buildtime.mjs times hot reload by adding an attribute to this element's opening tag
  // (its ROOT_TAG). When this element changes, change ROOT_TAG with it.
  return <div id="app-root" />;
}
