import type { HostTransport } from "./transport/host-transport.ts";

/** The whole app, for now: one empty root element. The transport is how it will reach the host. */
export function App(_props: { transport: HostTransport }) {
  return <div id="app-root" />;
}
