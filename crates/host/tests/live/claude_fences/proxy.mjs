// A recording HTTP proxy on 127.0.0.1 standing in for Clauseo's egress proxy (fence 4).
// Every request is logged as one JSON line. Only requests to `forward` (the fake API's
// host:port) are passed on, tagged `x-fence-proxy: 1`; everything else gets 403 and never
// leaves the machine. CONNECT to anything but `forward` is refused the same way.

import http from "node:http";
import net from "node:net";
import fs from "node:fs";

export class RecordingProxy {
  constructor({ log, forward }) {
    this.logPath = log;
    this.forward = forward; // "127.0.0.1:<port>"
    this.label = "";
  }

  log(entry) {
    fs.appendFileSync(this.logPath, JSON.stringify({ t: new Date().toISOString(), label: this.label, ...entry }) + "\n");
  }

  async start() {
    this.server = http.createServer((req, res) => this.plain(req, res));
    this.server.on("connect", (req, socket, head) => this.connect(req, socket, head));
    await new Promise((ok) => this.server.listen(0, "127.0.0.1", ok));
    this.port = this.server.address().port;
    return this;
  }

  stop() {
    this.server?.closeAllConnections?.();
    return new Promise((ok) => (this.server ? this.server.close(() => ok()) : ok()));
  }

  auth(req) {
    const header = req.headers["proxy-authorization"];
    if (!header) return null;
    const [, b64] = header.split(" ");
    const user = Buffer.from(b64 ?? "", "base64").toString().split(":")[0];
    return { user, tokenMatches: header === this.expectedAuth };
  }

  plain(req, res) {
    let target;
    try {
      target = new URL(req.url);
    } catch {
      this.log({ method: req.method, target: req.url, auth: this.auth(req), forwarded: false, note: "not a proxy request" });
      res.writeHead(400);
      return res.end("not a proxy request\n");
    }
    const forwarded = target.host === this.forward;
    const ua = req.headers["user-agent"];
    this.log({ method: req.method, target: req.url, auth: this.auth(req), forwarded, ua });
    if (!forwarded) {
      res.writeHead(403, { "content-type": "text/plain" });
      return res.end("blocked by the fence-proof recording proxy\n");
    }
    const headers = { ...req.headers, "x-fence-proxy": "1" };
    delete headers["proxy-authorization"];
    delete headers["proxy-connection"];
    const upstream = http.request(
      { host: target.hostname, port: target.port, method: req.method, path: target.pathname + target.search, headers },
      (up) => {
        res.writeHead(up.statusCode, up.headers);
        up.pipe(res);
      },
    );
    upstream.on("error", (error) => {
      res.writeHead(502);
      res.end(String(error));
    });
    req.pipe(upstream);
  }

  connect(req, socket, head) {
    const forwarded = req.url === this.forward;
    this.log({ method: "CONNECT", target: req.url, auth: this.auth(req), forwarded });
    if (!forwarded) {
      socket.end("HTTP/1.1 403 Forbidden\r\n\r\n");
      return;
    }
    const [host, port] = req.url.split(":");
    const up = net.connect(Number(port), host, () => {
      socket.write("HTTP/1.1 200 Connection Established\r\n\r\n");
      up.write(head);
      up.pipe(socket);
      socket.pipe(up);
    });
    up.on("error", () => socket.destroy());
    socket.on("error", () => up.destroy());
  }
}
