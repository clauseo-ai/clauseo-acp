// @vitest-environment happy-dom
// The text field takes its words two ways: uncontrolled, from defaultValue, or controlled, from value. Controlled, a
// keystroke reports the new text through onValueChange and the shown text stays what value says, so a store decides.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { TextField } from "./text-field.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

function render(node: React.ReactNode) {
  if (root === null) {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  }
  return act(async () => root?.render(node));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const field = () => document.querySelector<HTMLInputElement>("input");

async function type(input: HTMLInputElement, value: string) {
  await act(async () => {
    Reflect.set(HTMLInputElement.prototype, "value", value, input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

test("controlled: a keystroke reports the new text, and the shown text stays what value says", async () => {
  const onValueChange = vi.fn();
  await render(
    <TextField
      label="Folder"
      note="Where the matter lives"
      value="Old"
      onValueChange={onValueChange}
    />,
  );
  expect(field()?.value).toBe("Old");
  await type(field()!, "Older");
  expect(onValueChange).toHaveBeenCalledWith("Older", expect.anything());
  expect(field()?.value).toBe("Old");
});

test("controlled: a new value from the store shows; the text follows value", async () => {
  const keep = () => {};
  await render(
    <TextField label="Folder" note="Where the matter lives" value="Old" onValueChange={keep} />,
  );
  await render(
    <TextField label="Folder" note="Where the matter lives" value="Moved" onValueChange={keep} />,
  );
  expect(field()?.value).toBe("Moved");
});

test("uncontrolled: defaultValue starts the text and typing changes it without any value", async () => {
  await render(<TextField label="Folder" note="Where the matter lives" defaultValue="Start" />);
  expect(field()?.value).toBe("Start");
  await type(field()!, "Start, typed");
  expect(field()?.value).toBe("Start, typed");
});
