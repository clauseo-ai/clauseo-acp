// The states the registered strings show in: the real room (src/room/room.tsx) over a host scripted for the state.
import { useState } from "react";
import { Room } from "../room/room.tsx";
import type { FakeTransport } from "../transport/fake-transport.ts";

/** The room over the transport `host` builds, built once for the frame's life. */
export function RoomOver({ host }: { host: () => FakeTransport }) {
  const [transport] = useState(host);
  return <Room transport={transport} />;
}
