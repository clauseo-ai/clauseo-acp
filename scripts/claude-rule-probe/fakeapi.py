#!/usr/bin/env python3
r"""A fake Anthropic API for probing Claude Code's permission matcher offline.

Each /v1/messages request without a tool_result answers with one Bash (or PowerShell)
tool_use whose command is read from CMD_FILE (or, when the case has an "input", a use of its
"tool" with that input, e.g. Read with {"file_path": ...}); a request that carries a tool_result records
that result's text to OUT_FILE (prefixed "ERROR: " when it is an is_error result) and answers
with a short end_turn text.

Usage: fakeapi.py CMD_FILE OUT_FILE PORT [REQ_LOG]
With REQ_LOG (or the REQ_LOG env var), every POST is appended to it as one JSON line: path,
model, stream, the head of the system prompt, the tool names, and whether it is an auto mode
classifier request. Claude Code's local auto mode classifier (Vfe, Rdo in 2.1.293) sends
tool-less, non-streaming requests (model claude-sonnet-5) whose system prompt begins "You are a
security monitor for autonomous AI coding agents."; its last user block holds
"<transcript>...</transcript>" with the action. By stage it reads /<severity>\s*(\d+)/ (Tdo;
50 is the line, stage 1 stops at "</severity>") or /<block>(yes|no)\b/ (iFe). This server
answers both at once: "<block>no</block><severity>5</severity>" (allow), or a block verdict when
the env var CLASSIFIER_VERDICT=block.
"""
import json, os, sys, uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CMD_FILE, OUT_FILE, PORT = sys.argv[1], sys.argv[2], int(sys.argv[3])
REQ_LOG = sys.argv[4] if len(sys.argv) > 4 else os.environ.get("REQ_LOG")
CLASSIFIER_BLOCK = os.environ.get("CLASSIFIER_VERDICT", "allow").lower() == "block"
# SERVER_VERDICT=flagged|not_flagged: a tool_use response also carries the server-side auto mode
# classifier's verdict on that call (message_delta.delta.safeguard_results, read by hKt), as the
# first-party API does when auto mode runs its "server arbiter" (sO). Unset: no verdict, which
# Claude Code latches as "the server does not classify" for the rest of the session.
SERVER_VERDICT = os.environ.get("SERVER_VERDICT")


def texts(value):
    """The text of a system prompt or a message content (a string or a list of blocks)."""
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "".join(b.get("text", "") for b in value if isinstance(b, dict) and isinstance(b.get("text"), str))
    return ""


def is_classifier(body):
    """Claude Code's auto mode classifier request: its transcript and its <block> verdict format."""
    system = texts(body.get("system"))
    msgs = body.get("messages", [])
    first = texts(msgs[0].get("content")) if msgs and isinstance(msgs[0], dict) else ""
    every = system + "".join(texts(m.get("content")) for m in msgs if isinstance(m, dict))
    return ("You are a security monitor for autonomous AI coding agents" in system
            or first.lstrip().startswith("<transcript>") or "MUST begin with <block>" in every)


def log_request(path, body, classifier, headers=None):
    if not REQ_LOG:
        return
    entry = {"path": path, "model": body.get("model"), "stream": bool(body.get("stream")),
             "system": texts(body.get("system"))[:160],
             "first_user": next((texts(m.get("content"))[:160] for m in body.get("messages", [])
                                 if isinstance(m, dict) and m.get("role") == "user"), ""),
             "tools": [t.get("name") for t in body.get("tools", []) if isinstance(t, dict)],
             "max_tokens": body.get("max_tokens"), "stop_sequences": body.get("stop_sequences"),
             "keys": sorted(body), "beta": headers.get("anthropic-beta") if headers else None,
             "classifier": classifier}
    if classifier:
        entry["action"] = texts(body.get("messages", [{}])[-1].get("content"))[-400:]
    with open(REQ_LOG, "a") as f:
        f.write(json.dumps(entry) + "\n")


def sse(handler, events):
    handler.send_response(200)
    handler.send_header("content-type", "text/event-stream")
    handler.end_headers()
    for name, data in events:
        handler.wfile.write(f"event: {name}\ndata: {json.dumps(data)}\n\n".encode())
    handler.wfile.flush()


def plain(handler, blocks, stop):
    """A non-streaming Messages response."""
    content = [{"type": "text", "text": b["text"]} if b["type"] == "text" else
               {"type": "tool_use", "id": b["id"], "name": b["name"], "input": b["input"]} for b in blocks]
    data = {"id": "msg_" + uuid.uuid4().hex[:20], "type": "message", "role": "assistant",
            "model": "claude-sonnet-4-5", "content": content, "stop_reason": stop, "stop_sequence": None,
            "usage": {"input_tokens": 1, "output_tokens": 1}}
    handler.send_response(200)
    handler.send_header("content-type", "application/json")
    handler.end_headers()
    handler.wfile.write(json.dumps(data).encode())


def reply(handler, body, blocks, stop):
    if body.get("stream"):
        events = message(blocks, stop)
        uses = [b["id"] for b in blocks if b["type"] == "tool_use"]
        if SERVER_VERDICT and uses:
            for name, data in events:
                if name == "message_delta":
                    data["delta"]["safeguard_results"] = [{"type": "dangerous_tool_use", "status": {
                        "type": "available", "tool_uses": {u: {"type": "evaluated", "outcome": SERVER_VERDICT,
                        "explanation": "[Data Exfiltration]"} for u in uses}}}]
        sse(handler, events)
    else:
        plain(handler, blocks, stop)


def message(blocks, stop):
    mid = "msg_" + uuid.uuid4().hex[:20]
    start = {"type": "message_start", "message": {"id": mid, "type": "message", "role": "assistant",
             "model": "claude-sonnet-4-5", "content": [], "stop_reason": None,
             "stop_sequence": None, "usage": {"input_tokens": 1, "output_tokens": 1}}}
    events = [("message_start", start)]
    for i, block in enumerate(blocks):
        if block["type"] == "text":
            events.append(("content_block_start", {"type": "content_block_start", "index": i,
                           "content_block": {"type": "text", "text": ""}}))
            events.append(("content_block_delta", {"type": "content_block_delta", "index": i,
                           "delta": {"type": "text_delta", "text": block["text"]}}))
        else:
            events.append(("content_block_start", {"type": "content_block_start", "index": i,
                           "content_block": {"type": "tool_use", "id": block["id"],
                                             "name": block["name"], "input": {}}}))
            events.append(("content_block_delta", {"type": "content_block_delta", "index": i,
                           "delta": {"type": "input_json_delta",
                                     "partial_json": json.dumps(block["input"])}}))
        events.append(("content_block_stop", {"type": "content_block_stop", "index": i}))
    events.append(("message_delta", {"type": "message_delta",
                   "delta": {"stop_reason": stop, "stop_sequence": None},
                   "usage": {"output_tokens": 1}}))
    events.append(("message_stop", {"type": "message_stop"}))
    return events


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(b"{}")

    do_HEAD = do_GET

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length", 0))) or b"{}")
        if not self.path.startswith("/v1/messages") or "count_tokens" in self.path:
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"input_tokens": 1}).encode())
            return
        classifier = is_classifier(body)
        log_request(self.path, body, classifier, self.headers)
        if classifier:
            verdict = ("<block>yes</block><severity>95</severity><reason>probe says block</reason>"
                       if CLASSIFIER_BLOCK else "<block>no</block><severity>5</severity>")
            reply(self, body, [{"type": "text", "text": verdict}], "end_turn")
            return
        msgs = body.get("messages", [])
        tools = [t.get("name") for t in body.get("tools", []) if isinstance(t, dict)]
        users = [m for m in msgs if m.get("role") == "user"]
        last = users[-1] if users else {}
        results = []
        if isinstance(last.get("content"), list):
            for block in last["content"]:
                if isinstance(block, dict) and block.get("type") == "tool_result":
                    content = block.get("content")
                    if isinstance(content, list):
                        content = " ".join(c.get("text", "") for c in content if isinstance(c, dict))
                    # An is_error result (a refusal, or a command's non-zero exit) is marked.
                    results.append(("ERROR: " if block.get("is_error") else "") + str(content))
        if results:
            with open(OUT_FILE, "a") as f:
                f.write(json.dumps(results) + "\n")
            reply(self, body, [{"type": "text", "text": "done"}], "end_turn")
            return
        spec = json.load(open(CMD_FILE))
        if not tools:
            # A side request (a title, a classifier): answer it, record nothing.
            reply(self, body, [{"type": "text", "text": "ok"}], "end_turn")
            return
        if spec["tool"] not in tools:
            with open(OUT_FILE, "a") as f:
                f.write(json.dumps(["NO TOOL " + spec["tool"] + " in " + ",".join(map(str, tools))]) + "\n")
            reply(self, body, [{"type": "text", "text": "no tool"}], "end_turn")
            return
        block = {"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:20], "name": spec["tool"],
                 "input": spec.get("input") or {"command": spec["command"], "description": "probe"}}
        reply(self, body, [block], "tool_use")


ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
