// The WebKit check (U1): loads the room's probe build in a bare WKWebView, the engine the Mac app draws it in, at every
// width the room lays out for, in day and English and in night and Hindi. After each page's load event it asks the
// page a question every 50 ms (forcing a layout each time), so a page whose layout hangs simply never answers. One line
// per page and size; exits 1 if any page fails. The design's probe (design/03-system/scripts/webkit-probe.swift),
// pointed at the app's room.
//
// A page fails when it doesn't answer within 3 s of its load event, logs an error, an unhandled rejection, a console
// error or a CSP violation, or loses its content process. It answers once the room has its title, its wall's five
// tiles, and the display the query asked for on <html>. One more page per look freezes Object.prototype in the main
// frame first, as Tauri's freezePrototype does, and runs window.__clauseoAxe() there: it must resolve with no violation.
//
// Usage, from ui/app: pnpm build:probe, then serve dist-probe/ with `pnpm exec vite preview --mode probe` (port 4173),
// then pnpm probe:webkit. PROBE_ORIGIN names another server.
import Cocoa
import WebKit

let origin = ProcessInfo.processInfo.environment["PROBE_ORIGIN"] ?? "http://127.0.0.1:4173"
let budget = 3.0 // seconds from the load event to the first answer
let watch = 1.5 // seconds after the first answer to keep listening for errors
let loadLimit = 20.0 // seconds from the request to the load event

struct Run {
  let name: String
  let url: URL
  let width: CGFloat
  let height: CGFloat
  /// The page's own answer that it has rendered: a script that returns true.
  let rendered: String
  /// Run at document start, after the listeners, in the main frame only (as Tauri's own init scripts on the Mac).
  let setup: String
}

let sizes: [(CGFloat, CGFloat)] = [(390, 844), (1100, 800), (1280, 800), (1512, 900), (1920, 1080)]
let looks = [("day", "en"), ("night", "hi")]
var runs: [Run] = []
for (theme, lang) in looks {
  let q = "?theme=\(theme)&lang=\(lang)"
  // the title from list_matters, the five tiles from list_papers, and the asked-for display on <html>
  let rendered = "document.querySelector('#root main.room-centre') !== null" +
    " && (document.querySelector('[data-testid=\"matter\"]')?.textContent ?? '') !== ''" +
    " && document.querySelectorAll('#root aside.room-pane li.tile').length === 5" +
    " && document.documentElement.dataset.theme === '\(theme)' && document.documentElement.lang === '\(lang)'"
  for (w, h) in sizes {
    runs.append(Run(name: "room \(theme) \(lang)", url: URL(string: "\(origin)/\(q)")!, width: w, height: h,
                    rendered: rendered, setup: ""))
  }
  // Tauri's freezePrototype (tauri.conf.json), as the app runs: Object.prototype frozen in the main frame before any
  // page script. window.__clauseoAxe() must still load axe and run it over the page, colour contrast included, and
  // find nothing; a rejection or any violation fails the page.
  let axe = rendered + " && (window.__axeStarted || (window.__axeStarted = true, window.__clauseoAxe().then(" +
    "function (v) { window.__axeOut = v; v.forEach(function (x) { __probe.errs.push('axe: ' + x.id + ' ' + " +
    "x.nodes.map(function (n) { return n.target.join(' '); }).join(', ')); }); }, " +
    "function (e) { window.__axeOut = []; __probe.errs.push('axe rejected: ' + String(e && (e.stack || e.message) || e).slice(0, 300)); }), " +
    "false)) && Array.isArray(window.__axeOut)"
  runs.append(Run(name: "room \(theme) \(lang), frozen, axe", url: URL(string: "\(origin)/\(q)")!, width: 1280,
                  height: 800, rendered: axe, setup: "Object.freeze(Object.prototype);"))
}

// Listens for errors from the first moment, before the page's own scripts run.
let listen = """
window.__probe = { errs: [] };
addEventListener('error', function (e) {
  var t = e.target, where = t && t !== window && (t.src || t.href) ? ' loading ' + (t.src || t.href) : '';
  __probe.errs.push('error: ' + (e.message || e.type) + where + (e.filename ? ' @ ' + e.filename + ':' + e.lineno : ''));
}, true);
addEventListener('unhandledrejection', function (e) { __probe.errs.push('rejection: ' + String(e.reason && (e.reason.stack || e.reason.message) || e.reason).slice(0, 300)); });
document.addEventListener('securitypolicyviolation', function (e) { __probe.errs.push('csp: ' + e.violatedDirective + ' ' + e.blockedURI); });
(function () { var ce = console.error.bind(console); console.error = function () { __probe.errs.push('console.error: ' + Array.prototype.map.call(arguments, String).join(' ').slice(0, 300)); ce.apply(null, arguments); }; })();
"""

final class Probe: NSObject, WKNavigationDelegate {
  var queue: [Run]
  var web: WKWebView?
  var current: Run?
  var seq = 0
  var asked = Date()
  var loaded: Date?
  var answeredAt: Int?
  var nodes = 0
  var failures = 0
  var lines: [String] = []

  init(_ runs: [Run]) { queue = runs }

  func next() {
    web?.navigationDelegate = nil
    web = nil
    guard !queue.isEmpty else { finish() }
    let run = queue.removeFirst()
    seq += 1
    let mine = seq
    current = run
    loaded = nil
    answeredAt = nil
    let cfg = WKWebViewConfiguration()
    cfg.websiteDataStore = .nonPersistent()
    let ucc = WKUserContentController()
    ucc.addUserScript(WKUserScript(source: listen + run.setup, injectionTime: .atDocumentStart, forMainFrameOnly: true))
    cfg.userContentController = ucc
    let view = WKWebView(frame: NSRect(x: 0, y: 0, width: run.width, height: run.height), configuration: cfg)
    view.navigationDelegate = self
    web = view
    asked = Date()
    view.load(URLRequest(url: run.url, cachePolicy: .reloadIgnoringLocalCacheData))
    DispatchQueue.main.asyncAfter(deadline: .now() + loadLimit) {
      if self.seq == mine && self.loaded == nil { self.fail("no load event in \(Int(loadLimit)) s") }
    }
  }

  func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
    guard webView === web, loaded == nil else { return }
    loaded = Date()
    let mine = seq
    DispatchQueue.main.asyncAfter(deadline: .now() + budget) {
      if self.seq == mine && self.answeredAt == nil { self.fail("no answer within \(Int(budget)) s of the load event: hung") }
    }
    poll(mine)
  }

  /// Asks whether the page has rendered, forcing a layout; asks again 50 ms after each answer until it has.
  func poll(_ mine: Int) {
    guard seq == mine, let run = current, let view = web else { return }
    let js = "document.documentElement.getBoundingClientRect(); void document.body.offsetHeight; return (\(run.rendered)) ? document.querySelectorAll('*').length : 0;"
    view.callAsyncJavaScript(js, arguments: [:], in: nil, in: .page) { result in
      guard self.seq == mine, let loaded = self.loaded else { return }
      if case .success(let v) = result, let n = v as? Int, n > 0 {
        self.answeredAt = Int(Date().timeIntervalSince(loaded) * 1000)
        self.nodes = n
        DispatchQueue.main.asyncAfter(deadline: .now() + watch) { self.report(mine) }
      } else if case .failure(let e) = result {
        self.fail("could not ask the page: \(e.localizedDescription)")
      } else {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { self.poll(mine) }
      }
    }
  }

  /// After listening a while longer: anything the page logged, and whether it still answers.
  func report(_ mine: Int) {
    guard seq == mine, let view = web else { return }
    var back = false
    view.callAsyncJavaScript("void document.body.offsetHeight; return JSON.stringify(window.__probe ? __probe.errs : ['the probe was not installed']);",
                             arguments: [:], in: nil, in: .page) { result in
      guard self.seq == mine else { return }
      back = true
      var errs: [String] = []
      if case .success(let v) = result, let s = v as? String, let data = s.data(using: .utf8),
         let list = try? JSONSerialization.jsonObject(with: data) as? [String] { errs = list }
      if errs.isEmpty { self.pass() } else { self.fail(errs.prefix(4).joined(separator: " | ")) }
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + budget) {
      if self.seq == mine && !back { self.fail("answered once, then stopped answering: hung") }
    }
  }

  func label() -> String {
    guard let run = current else { return "" }
    let size = "\(Int(run.width))x\(Int(run.height))"
    return "\(run.name.padding(toLength: 30, withPad: " ", startingAt: 0)) \(size.padding(toLength: 9, withPad: " ", startingAt: 0))"
  }

  func pass() {
    print("ok    \(label()) \(answeredAt.map { "\($0) ms" } ?? ""), \(nodes) elements")
    fflush(stdout)
    next()
  }

  func fail(_ why: String) {
    failures += 1
    let t = answeredAt.map { "\($0) ms" } ?? "no answer"
    print("FAIL  \(label()) \(t): \(why)")
    fflush(stdout)
    next()
  }

  func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
    guard webView === web else { return }
    fail("its content process was lost")
  }

  func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
    guard webView === web else { return }
    fail("could not load: \(error.localizedDescription)")
  }

  func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
    guard webView === web else { return }
    fail("failed while loading: \(error.localizedDescription)")
  }

  func finish() -> Never {
    print(failures == 0 ? "webkit probe: all \(seq) pages answered within \(Int(budget)) s, with no errors" : "webkit probe: \(failures) of \(seq) failed")
    exit(failures == 0 ? 0 : 1)
  }
}

print("webkit probe: \(runs.count) pages from \(origin), \(Int(budget)) s budget from each load event")
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let probe = Probe(runs)
DispatchQueue.main.async { probe.next() }
// a last resort, should the probe itself stall
DispatchQueue.main.asyncAfter(deadline: .now() + Double(runs.count) * (loadLimit + 2 * budget + watch) + 30) {
  print("webkit probe: stalled; giving up")
  exit(1)
}
app.run()
