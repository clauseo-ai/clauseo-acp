import { afterEach, describe, expect, test, vi } from "vitest";
import { referenceOf } from "@clauseo/components/markdown/schema.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { Papers } from "./host-next.ts";
import { AnswerHost, quietly, type ListedPapers } from "./open.ts";

/** A host for `matter` that has been shown `shown`. */
const answerHost = (t: FakeTransport, matter: string, shown: () => ListedPapers) => {
  const host = new AnswerHost(t, matter);
  host.show(shown());
  return host;
};

const FILE = "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
const papers: Papers = {
  papers: [
    { file: FILE, path: "drafts/lease.docx", size: 10, modified: 0 },
    { file: "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5c", path: "README", size: 1, modified: 0 },
  ],
  skipped: [],
  unread: [],
  truncated: false,
};
const listed: ListedPapers = { matter: "m-1", papers };
const flush = () => new Promise((done) => setTimeout(done, 0));
const declined = { code: "transport.unauthorized", message: "declined", retryable: false };

afterEach(() => vi.restoreAllMocks());

describe("AnswerHost", () => {
  test("a link goes to openExternal", async () => {
    const t = new FakeTransport();
    answerHost(t, "m-1", () => listed).openExternal("https://example.com/x");
    await flush();
    expect(t.only("open_external")).toEqual([
      { call: "open_external", url: "https://example.com/x" },
    ]);
  });

  test("a paper opens by its file ref, or by its exact path; a citation does nothing", async () => {
    const t = new FakeTransport();
    const host = answerHost(t, "m-1", () => listed);
    host.openReference("paper", FILE);
    host.openReference("paper", "README");
    host.openReference("citation", FILE);
    await flush();
    expect(t.only("open_file").map((c) => c.file)).toEqual([
      FILE,
      "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5c",
    ]);
  });

  test("a clauseo-paper: reference naming a paper's file ref passes the renderer's id rule and opens that file", async () => {
    const reference = referenceOf(`clauseo-paper:${FILE}`);
    expect(reference).toEqual({ kind: "paper", id: FILE });
    if (reference === null || reference === "invalid") return;
    const t = new FakeTransport();
    answerHost(t, "m-1", () => listed).openReference(reference.kind, reference.id);
    await flush();
    expect(t.only("open_file")).toEqual([{ call: "open_file", file: FILE }]);
  });

  test("no papers, another matter's papers or no match: the click does nothing", async () => {
    const t = new FakeTransport();
    answerHost(t, "m-1", () => null).openReference("paper", FILE);
    answerHost(t, "m-2", () => listed).openReference("paper", FILE);
    answerHost(t, "m-1", () => listed).openReference("paper", "lease");
    await flush();
    expect(t.only("open_file")).toEqual([]);
  });

  test("a declined confirm is silent; another refusal goes to the console only", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const t = new FakeTransport({
      openExternal: () => {
        throw declined;
      },
      openFile: () => {
        throw { code: "matter.folder_missing", message: "gone", retryable: false };
      },
    });
    const host = answerHost(t, "m-1", () => listed);
    host.openExternal("https://example.com");
    host.openReference("paper", FILE);
    await flush();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0])).toContain("open_file failed");
  });
});

describe("quietly", () => {
  test("resolves whatever the call did", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    await expect(quietly("reveal", Promise.reject(declined))).resolves.toBeUndefined();
    await expect(quietly("reveal", Promise.reject("ipc down"))).resolves.toBeUndefined();
    await expect(quietly("reveal", Promise.resolve())).resolves.toBeUndefined();
  });
});
