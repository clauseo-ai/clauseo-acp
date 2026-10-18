import type { HostTransport } from "./transport/host-transport.ts";

/** The whole app, for now: one empty root element. The transport is how it will reach the host. */
export function App(_props: { transport: HostTransport }) {
  // scripts/buildtime.mjs times hot reload by editing this exact text, `<div id="app-root"`. When
  // this element changes, change the script's ROOT_TAG with it.
  return <div id="app-root" />;
}
