// The room's parts, in the order the room arranges them: the chrome over the matter, the thread and what it holds (its
// step groups among it), the composer and the lines over it, then the room's states.
import { Group } from "./frame.tsx";
import { ChromeSamples } from "./room-chrome.tsx";
import { InputSamples } from "./room-input.tsx";
import { StateSamples } from "./room-state.tsx";
import { StepSamples } from "./room-steps.tsx";
import { ThreadSamples } from "./room-thread.tsx";

export function RoomParts() {
  return (
    <Group title="The room">
      <ChromeSamples />
      <ThreadSamples />
      <StepSamples />
      <InputSamples />
      <StateSamples />
    </Group>
  );
}
