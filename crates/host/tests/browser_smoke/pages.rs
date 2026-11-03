//! The pages Chrome opens: server A under `smoke.test:<port>`, server B with the pixel on
//! `127.0.0.1`, and the proxy in front of both.

use std::net::TcpListener;
use std::sync::Arc;

use super::http::{self, Request, Response, Shared};

const HOME_PAGE: &str = "<!doctype html><title>Clauseo smoke</title><h1>Clauseo smoke</h1>";

/// Gathers ICE candidates with a public STUN server for at most 5 s, then posts them.
/// `PORT_A` and `PORT_B` are replaced with the page and pixel servers' ports.
const WEBRTC_PAGE: &str = r"<!doctype html><title>Clauseo smoke WebRTC</title>
<img src='http://127.0.0.1:PORT_B/pixel'>
<script>
const report = (list) => fetch('http://smoke.test:PORT_A/report',
  {method: 'POST', body: JSON.stringify(list)});
(async () => {
  const candidates = [];
  const pc = new RTCPeerConnection({iceServers: [{urls: 'stun:stun.l.google.com:19302'}]});
  pc.createDataChannel('smoke');
  pc.onicecandidate = (e) => { if (e.candidate && e.candidate.candidate) candidates.push(e.candidate.candidate); };
  const done = new Promise((resolve) => {
pc.onicegatheringstatechange = () => { if (pc.iceGatheringState === 'complete') resolve(); };
setTimeout(resolve, 5000);
  });
  await pc.setLocalDescription(await pc.createOffer());
  await done;
  for (const line of (pc.localDescription.sdp || '').split(/\r?\n/)) {
if (line.startsWith('a=candidate:')) candidates.push(line.slice(2));
  }
  await report(candidates);
})().catch((e) => report(['error: ' + e]));
</script>";

/// A 1x1 transparent GIF.
const PIXEL: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";

/// The proxy and the two page servers, and what they saw.
pub(crate) struct World {
    pub(crate) proxy_port: u16,
    pub(crate) port_a: u16,
    pub(crate) port_b: u16,
    pub(crate) log: Shared<Vec<String>>,
    pub(crate) report: Shared<Option<String>>,
    /// One entry per pixel request: whether it came through the proxy.
    pub(crate) pixel: Shared<Vec<bool>>,
}

fn bind() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local port");
    let port = listener.local_addr().expect("local address").port();
    (listener, port)
}

impl World {
    pub(crate) fn start() -> Self {
        let (listener_a, port_a) = bind();
        let (listener_b, port_b) = bind();
        let (listener_p, proxy_port) = bind();
        let world = Self {
            proxy_port,
            port_a,
            port_b,
            log: Arc::default(),
            report: Arc::default(),
            pixel: Arc::default(),
        };
        let host = format!("smoke.test:{port_a}");
        let webrtc = WEBRTC_PAGE
            .replace("PORT_A", &port_a.to_string())
            .replace("PORT_B", &port_b.to_string());
        let report = Arc::clone(&world.report);
        http::serve(listener_a, move |request: &Request| {
            if request.header("host") != Some(host.as_str()) {
                return Response::not_found();
            }
            match request.target.as_str() {
                "/" => Response::ok("text/html", HOME_PAGE),
                "/webrtc" => Response::ok("text/html", webrtc.clone()),
                "/report" => {
                    let body = String::from_utf8_lossy(&request.body).into_owned();
                    *report.lock().expect("report") = Some(body);
                    Response::ok("text/plain", "ok")
                }
                _ => Response::not_found(),
            }
        });
        let pixel = Arc::clone(&world.pixel);
        http::serve(listener_b, move |request: &Request| {
            if request.target != "/pixel" {
                return Response::not_found();
            }
            let via_proxy = request.header("x-smoke-proxy") == Some("1");
            pixel.lock().expect("pixel").push(via_proxy);
            Response::ok("image/gif", PIXEL)
        });
        http::proxy(listener_p, port_a, Arc::clone(&world.log));
        world
    }

    pub(crate) fn log(&self) -> Vec<String> {
        self.log.lock().expect("proxy log").clone()
    }
}
