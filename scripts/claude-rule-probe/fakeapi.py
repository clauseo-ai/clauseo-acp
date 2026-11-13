#!/usr/bin/env python3
"""A fake Anthropic API for probing Claude Code's permission matcher offline.

Each /v1/messages request without a tool_result answers with one Bash (or PowerShell)
tool_use whose command is read from CMD_FILE; a request that carries a tool_result records
that result's text to OUT_FILE and answers with a short end_turn text.
"""
import json, os, sys, uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CMD_FILE, OUT_FILE, PORT = sys.argv[1], sys.argv[2], int(sys.argv[3])


def sse(handler, events):
    handler.send_response(200)
    handler.send_header("content-type", "text/event-stream")
    handler.end_headers()
    for name, data in events:
        handler.wfile.write(f"event: {name}\ndata: {json.dumps(data)}\n\n".encode())
    handler.wfile.flush()


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
                    results.append(str(content))
        if results:
            with open(OUT_FILE, "a") as f:
                f.write(json.dumps(results) + "\n")
            sse(self, message([{"type": "text", "text": "done"}], "end_turn"))
            return
        spec = json.load(open(CMD_FILE))
        if not tools:
            # A side request (a title, a classifier): answer it, record nothing.
            sse(self, message([{"type": "text", "text": "ok"}], "end_turn"))
            return
        if spec["tool"] not in tools:
            with open(OUT_FILE, "a") as f:
                f.write(json.dumps(["NO TOOL " + spec["tool"] + " in " + ",".join(map(str, tools))]) + "\n")
            sse(self, message([{"type": "text", "text": "no tool"}], "end_turn"))
            return
        block = {"type": "tool_use", "id": "toolu_" + uuid.uuid4().hex[:20], "name": spec["tool"],
                 "input": {"command": spec["command"], "description": "probe"}}
        sse(self, message([block], "tool_use"))


ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
