// @vitest-environment happy-dom
// The one rule for where a dialog's focus starts: on the act, the dark primary, when it can be undone; on the safe
// choice when it can't. Either way the starting focus draws no ring until a key is pressed in the dialog.
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { AlertDialog, AlertDialogClose, AlertDialogPopup } from "./alert-dialog.tsx";
import { Button } from "./button.tsx";
import { Dialog, DialogClose, DialogPopup } from "./dialog.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot> | null = null;
const tick = () => act(async () => new Promise<void>((r) => setTimeout(r, 80)));

async function show(node: React.ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<StrictMode>{node}</StrictMode>));
  await tick();
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

test("an act that can be undone takes the starting focus, with no ring until a key is pressed", async () => {
  await show(
    <Dialog open>
      <DialogPopup
        undoable
        title="Restore the 3:58 pm version?"
        actions={
          <>
            <DialogClose render={<Button kind="quiet" />}>Cancel</DialogClose>
            <Button kind="primary">Restore</Button>
          </>
        }
      />
    </Dialog>,
  );
  expect(document.activeElement?.textContent).toBe("Restore");
  const plane = document.querySelector("[data-dialog-plane]");
  expect(plane?.hasAttribute("data-quiet-start")).toBe(true);
  await act(async () => {
    plane?.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
  });
  expect(plane?.hasAttribute("data-quiet-start")).toBe(false);
});

test("an act that can't be undone leaves the starting focus on the safe choice", async () => {
  await show(
    <AlertDialog open>
      <AlertDialogPopup
        undoable={false}
        title="Switch off protection?"
        actions={
          <>
            <AlertDialogClose render={<Button kind="tone" />}>Keep protection</AlertDialogClose>
            <Button kind="quiet">Switch off</Button>
          </>
        }
      />
    </AlertDialog>,
  );
  expect(document.activeElement?.textContent).toBe("Keep protection");
});
