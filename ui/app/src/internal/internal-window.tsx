// The plain internal window: pick a folder for a matter, open a thread, send a prompt, watch the answer stream with
// its tool calls, answer permissions, cancel, undo a turn's files, see a usage-limit stop. Never shown to a lawyer;
// the new design replaces it. Every control has a stable `data-testid` for the e2e specs (ui/e2e/app-windows).
import { useCallback, useState, type KeyboardEvent } from "react";
import { useStreamError, useStreamPart, useViewStore, useWatch } from "../store/hooks.ts";
import { entryId, streamOf, type StreamView } from "../store/view.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { StreamId } from "../transport/protocol.ts";
import { ThreadContext, useSession, type Session } from "./session.ts";
import { Transcript } from "./transcript.tsx";
import { errorCopy, turnRunning } from "./words.ts";

const bar = {
  display: "flex",
  gap: 8,
  alignItems: "center",
  padding: 8,
  borderBottom: "1px solid #ddd",
} as const;

/** Whether Send waits: a turn runs, or the prompt just sent has no step or limit stop yet. */
function useBusy(stream: StreamId | null, sent: Session["sentTurn"]): boolean {
  const store = useViewStore();
  const pick = useCallback(
    (v: StreamView | undefined) => {
      if (turnRunning(v?.header?.header?.status)) return true;
      if (sent === null) return false;
      // Until the view holds the sent turn's end, the turn counts as running.
      return (
        v === undefined ||
        !(v.entries.has(entryId("step", sent)) || v.entries.has(entryId("limit_stop", sent)))
      );
    },
    [sent],
  );
  return useStreamPart(store, stream, pick);
}

function Composer({ session, stream }: { session: Session; stream: StreamId | null }) {
  const [draft, setDraft] = useState("");
  const busy = useBusy(stream, session.sentTurn);
  const canSend = stream !== null && !busy && draft.trim() !== "";
  const send = () => {
    if (!canSend) return;
    const text = draft;
    setDraft("");
    // send never rejects: a failure shows as the session's error and puts the draft back.
    void session.send(text).then((ok) => {
      if (!ok) setDraft((d) => (d === "" ? text : d));
    });
  };
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      send();
    }
  };
  return (
    <div style={{ ...bar, borderTop: "1px solid #ddd", borderBottom: 0, alignItems: "flex-end" }}>
      <textarea
        data-testid="composer"
        aria-label="Prompt"
        rows={3}
        style={{ flex: 1, font: "inherit" }}
        value={draft}
        disabled={stream === null}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={onKey}
      />
      <button type="button" data-testid="send" disabled={!canSend} onClick={send}>
        Send
      </button>
      {busy && (
        <button type="button" data-testid="cancel" onClick={session.cancel}>
          Cancel
        </button>
      )}
    </div>
  );
}

/** Why the open thread's stream stopped, by the error's code, with a way to ask for it again. */
function StreamError({ stream }: { stream: StreamId }) {
  const store = useViewStore();
  const error = useStreamError(store, stream);
  if (error === null) return null;
  return (
    <p
      role="alert"
      data-testid="stream-error"
      data-code={error.code}
      style={{ margin: 0, padding: 8, background: "#fdecec" }}
    >
      {errorCopy(error)}{" "}
      <button type="button" data-testid="stream-retry" onClick={() => store.retry(stream)}>
        Try again
      </button>
    </p>
  );
}

function ThreadView({ session, stream }: { session: Session; stream: StreamId }) {
  useWatch(useViewStore(), stream);
  const thread = session.thread!;
  return (
    <ThreadContext value={{ thread, stream, answer: session.answer, undo: session.undo }}>
      <StreamError stream={stream} />
      <Transcript stream={stream} />
    </ThreadContext>
  );
}

export function InternalWindow({ transport }: { transport: HostTransport }) {
  const session = useSession(transport);
  const [name, setName] = useState("Matter");
  const stream = session.thread === null ? null : streamOf(session.thread);
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        fontFamily: "system-ui, sans-serif",
      }}
    >
      <div style={bar}>
        <input
          data-testid="matter-name"
          aria-label="Matter name"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <button
          type="button"
          data-testid="new-matter"
          disabled={name.trim() === ""}
          onClick={() => session.newMatter(name.trim())}
        >
          Pick folder…
        </button>
        <span data-testid="matter" style={{ fontWeight: 600 }}>
          {session.matter?.name ?? (session.opening ? "Opening…" : "No matter")}
        </span>
        {session.matter !== null && (
          <>
            <select
              data-testid="thread-select"
              aria-label="Thread"
              value={session.thread ?? ""}
              onChange={(e) => session.openThread(e.target.value)}
            >
              {session.thread === null && <option value="">No thread</option>}
              {session.threads.map((t, i) => (
                <option key={t.thread} value={t.thread}>
                  {t.title ?? `Thread ${i + 1}`}
                </option>
              ))}
            </select>
            <button type="button" data-testid="new-thread" onClick={session.newThread}>
              New thread
            </button>
          </>
        )}
      </div>
      {session.error !== null && (
        <p
          role="alert"
          data-testid="error"
          data-code={session.error.code}
          style={{ margin: 0, padding: 8, background: "#fdecec" }}
        >
          {errorCopy(session.error)}
        </p>
      )}
      {stream !== null ? (
        <ThreadView key={stream} session={session} stream={stream} />
      ) : (
        <div style={{ flex: 1 }} />
      )}
      <Composer session={session} stream={stream} />
    </div>
  );
}
