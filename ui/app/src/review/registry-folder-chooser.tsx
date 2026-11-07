// Clauseo's own folder chooser's copy review entries (FC-01 to FC-17): the chooser Windows gets when its folder window
// would stall, in the real room over FakeTransport, opened by New matter's Change over the scripted machine
// (transport/fake-chooser.ts), at the last place or where each state shows; in a 640 by 480 window, the smallest the
// chooser must fit. Apart from registry.tsx to keep that file under the size limit.
import type { ReactNode } from "react";
import type { MatterKey } from "@clauseo/components/lib/words-matters.ts";
import { FAKE_CHOOSER, type FakeChooser, type FakePath } from "../transport/fake-chooser.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { Chooser } from "./room-chooser.tsx";
import { matterWords } from "./words.ts";

/** The chooser opened by Change, at the last place `last`, the confirm answered by `choose` when given. */
const opened = (last: FakePath, choose?: FakeChooser["choose"]) => () =>
  new FakeTransport({
    query: () => ({ matters: [MATTER] }),
    chooser: { ...FAKE_CHOOSER, last, ...(choose === undefined ? {} : { choose }) },
  });

const MATTERS: FakePath = ["C:", "Users", "rmehra", "Documents", "Matters"];
const atMatters = () => <Chooser host={opened(MATTERS)} />;
/** The typed location filled in, as the lawyer types it. */
const typed = () => (
  <Chooser
    host={opened(MATTERS)}
    then={() => {
      const field = document.querySelector<HTMLInputElement>('[data-testid="chooser-typed"]');
      if (field === null) return;
      Reflect.set(HTMLInputElement.prototype, "value", "D:\\Cases\\Acme Traders", field);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    }}
  />
);
/** Choose this folder pressed, the desktop's confirm up and never answered. */
const waiting = () => (
  <Chooser
    host={opened(MATTERS, () => new Promise(() => {}))}
    then={() => document.querySelector<HTMLElement>('[data-testid="chooser-choose"]')?.click()}
  />
);

const entry = (id: string, key: MatterKey, render: () => ReactNode): Entry => ({
  id,
  words: matterWords(key),
  screen: "matters/folder-chooser",
  width: 640,
  height: 480,
  render,
});

export const FOLDER_CHOOSER: Entry[] = [
  entry("FC-01", "Choose the matter's folder", atMatters),
  entry(
    "FC-02",
    "Windows' folder window is slow to open on this computer, so Clauseo shows its own.",
    atMatters,
  ),
  // the start points beside the drives
  entry("FC-03", "Home", atMatters),
  entry("FC-04", "Desktop", atMatters),
  entry("FC-05", "Last place", atMatters),
  entry("FC-06", "Up", atMatters),
  entry("FC-07", "Or type a folder's location", typed),
  entry("FC-08", "Go", typed),
  entry("FC-09", "Choose this folder", atMatters),
  entry("FC-10", "Cancel", atMatters),
  // a OneDrive folder that doesn't answer in time, and its Try again
  entry("FC-11", "This folder didn't answer in time.", () => (
    <Chooser host={opened(["C:", "Users", "rmehra", "OneDrive - Iyer & Co"])} />
  )),
  entry("FC-12", "Try again", () => (
    <Chooser host={opened(["C:", "Users", "rmehra", "OneDrive - Iyer & Co"])} />
  )),
  entry("FC-13", "This folder can't be found.", () => (
    <Chooser host={opened(["C:", "Users", "rmehra", "Documents", "Moved away"])} />
  )),
  entry("FC-14", "Clauseo can't use this folder.", () => (
    <Chooser host={opened(["C:", "Windows"])} />
  )),
  // C:\, where Program Files and Windows are refused
  entry("FC-15", "Can't be a matter", () => <Chooser host={opened(["C:"])} />),
  entry("FC-16", "Some folders aren't shown.", () => <Chooser host={opened(["D:", "Archive"])} />),
  entry("FC-17", "Waiting for you to confirm…", waiting),
];
