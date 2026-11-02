// The WebKit check (U1): loads the room's probe build in a bare WKWebView, the engine the Mac app draws it in, at every
// width the room lays out for, in day and English and in night and Hindi. After each page's load event it asks the
// page a question every 50 ms (forcing a layout each time), so a page whose layout hangs simply never answers. One line
// per page and size; exits 1 if any page fails. The design's probe (design/03-system/scripts/webkit-probe.swift),
// pointed at the app's room.
//
// A page fails when it doesn't answer within 3 s of its load event, logs an error, an unhandled rejection, a console
// error or a CSP violation, or loses its content process. It answers once the room has its title, its wall's five
// tiles, and the display the query asked for on <html>; then it is measured: the page never scrolls sideways, the title
// is never cut off, and each row of file tiles is as tall as its tallest title and the tile's padding, with no room kept
// for a numeral or a chip. One more page per look freezes Object.prototype in the main frame first, as Tauri's
// freezePrototype does, and runs window.__clauseoAxe() there: it must resolve with no violation. The room with no
// matters (?matters=none: "No matters yet" as its title, the empty rail) is loaded in night Hindi at text sizes 16
// and 20. The rail in every state (?rail=states) is loaded at every width in both looks, folded and opened at 1100 px,
// and run through axe, with and without the new matter dialog open; there the find field must show (80 px or more),
// the moved row its ring, the form's folder line one line, and the scratch matter, opened, its raised row. The
// agent's permission requests (?thread=permission) are loaded at every width in both looks and run through axe: the
// page answers once it also shows a pending prompt with its four answers above one answered once, and neither prompt
// scrolls sideways. With ?confirm=1 as well, Always for this matter is pressed and held, and the page answers once the
// prompt is confirming (data-confirming and its OS line), at 1280 px and under the frozen axe run.
//
// Usage, from ui/app: pnpm build:probe, then serve dist-probe/ with `pnpm exec vite preview --mode probe` (port 4173),
// then pnpm probe:webkit. PROBE_ORIGIN names another server; PROBE_SHOTS names a folder to save a PNG of each page
// that passes, as WKWebView draws it.
import Cocoa
import WebKit

let origin = ProcessInfo.processInfo.environment["PROBE_ORIGIN"] ?? "http://127.0.0.1:4173"
let shots = ProcessInfo.processInfo.environment["PROBE_SHOTS"].map { URL(fileURLWithPath: $0, isDirectory: true) }
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
  /// Once rendered: an expression giving what is wrong with the page's layout, as a list of lines.
  var measure: String = measured
}

/// What every page is measured for once rendered: no sideways scroll, a title that isn't cut off, and rows of file
/// tiles only as tall as their tallest title and the tile's padding, within 4 px: a Latin file name in Hindi keeps
/// Hindi's line box, 2 px taller; the room of a numeral and a chip would add 52.
let measured = """
(function () {
  var errs = [], root = document.documentElement;
  if (root.scrollWidth > innerWidth) errs.push('layout: the page scrolls sideways, ' + root.scrollWidth + ' px in ' + innerWidth);
  var title = document.querySelector('[data-testid="matter"]');
  if (title && (title.scrollWidth > title.clientWidth + 1 || title.scrollHeight > title.clientHeight + 1))
    errs.push('layout: the title is cut off, ' + title.scrollWidth + 'x' + title.scrollHeight + ' in ' + title.clientWidth + 'x' + title.clientHeight);
  var rows = {};
  document.querySelectorAll('#root aside.room-pane li.tile > .tile-face').forEach(function (face) {
    var top = Math.round(face.getBoundingClientRect().top);
    (rows[top] = rows[top] || []).push(face);
  });
  Object.keys(rows).forEach(function (top) {
    var faces = rows[top], css = getComputedStyle(faces[0]);
    var pad = parseFloat(css.paddingTop) + parseFloat(css.paddingBottom);
    var tallest = Math.max.apply(null, faces.map(function (f) { return f.querySelector('.tile-title').getBoundingClientRect().height; }));
    var height = faces[0].getBoundingClientRect().height;
    if (Math.abs(height - (tallest + pad)) > 4)
      errs.push('layout: a row of tiles is ' + height.toFixed(1) + ' px, its title and padding ' + (tallest + pad).toFixed(1));
  });
  return errs;
})()
"""

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
// no matters: "No matters yet" in the title's place, in Hindi, at text sizes 16 and 20, and no wall of papers
for size in [16, 20] {
  let q = "?matters=none&theme=night&lang=hi&size=\(size)"
  let rendered = "document.querySelector('#root main.room-centre') !== null" +
    " && document.querySelector('[data-testid=\"matter\"]')?.lang === 'hi'" +
    " && (document.querySelector('[data-testid=\"matter\"]')?.textContent ?? '') !== ''" +
    " && document.querySelectorAll('#root aside.room-pane li.tile').length === 0" +
    " && document.documentElement.dataset.theme === 'night' && document.documentElement.lang === 'hi'"
  for (w, h) in sizes {
    runs.append(Run(name: "no matters night hi \(size)", url: URL(string: "\(origin)/\(q)")!, width: w, height: h,
                    rendered: rendered, setup: ""))
  }
}
// the empty rail on the same pages: New matter at its head, its empty state, and no matter row
for i in runs.indices where runs[i].name.hasPrefix("no matters") {
  runs[i] = Run(name: runs[i].name, url: runs[i].url, width: runs[i].width, height: runs[i].height,
                rendered: runs[i].rendered + " && document.querySelector('nav.room-rail [data-testid=\"new-matter\"]') !== null" +
                  " && document.querySelectorAll('nav.room-rail [data-testid=\"matter-row\"]').length === 0",
                setup: "")
}

// the rail in every state (?rail=states, probe-main.tsx): eleven matter rows over every date group, Pinned, and the
// scratch matter at the foot, measured as every page is and for a rail that never scrolls sideways; at 1100 px the rail
// folded behind Matters, opened as the floating panel; and axe over the rail and over the open new matter dialog, under
// a frozen Object.prototype
let railMeasured = "(function () { var errs = " + measured + ", rail = document.querySelector('nav.room-rail');" +
  " if (rail && rail.clientWidth > 0 && rail.scrollWidth > rail.clientWidth + 1)" +
  " errs.push('layout: the rail scrolls sideways, ' + rail.scrollWidth + ' px in ' + rail.clientWidth);" +
  // the find field shows its words wherever the rail stands: a field folded to its magnifier fails
  " var find = rail && rail.clientWidth > 0 ? rail.querySelector('input[type=search]') : null;" +
  " if (find && find.getBoundingClientRect().width < 80)" +
  " errs.push('layout: the find field is ' + find.getBoundingClientRect().width.toFixed(0) + ' px wide');" +
  // the new matter form's folder line: its icon, words and Change on one line (matter-form.css loaded)
  " var line = document.querySelector('[role=\"dialog\"] [data-testid=\"matter-existing\"]');" +
  " if (line && line.parentElement && line.parentElement.getBoundingClientRect().height > 60)" +
  " errs.push('layout: the folder line is ' + line.parentElement.getBoundingClientRect().height.toFixed(0) + ' px tall');" +
  // the moved folder's ring (matters.css loaded)
  " var ring = rail && rail.querySelector('[data-state=\"moved\"] .room-dot-ring');" +
  " if (rail && rail.querySelector('[data-state=\"moved\"]') && !(ring && /inset/.test(getComputedStyle(ring).boxShadow)))" +
  " errs.push('layout: the moved row has no ring');" +
  " return errs; })()"
let railRendered = { (theme: String, lang: String) in
  "document.querySelector('#root main.room-centre') !== null" +
    " && (document.querySelector('[data-testid=\"matter\"]')?.textContent ?? '') !== ''" +
    " && document.querySelectorAll('nav.room-rail [data-testid=\"matter-row\"]').length === 13" +
    " && document.querySelector('nav.room-rail [data-kind=\"scratch\"]') !== null" +
    " && document.documentElement.dataset.theme === '\(theme)' && document.documentElement.lang === '\(lang)'"
}
let axeRun = " && (window.__axeStarted || (window.__axeStarted = true, window.__clauseoAxe().then(" +
  "function (v) { window.__axeOut = v; v.forEach(function (x) { __probe.errs.push('axe: ' + x.id + ' ' + " +
  "x.nodes.map(function (n) { return n.target.join(' '); }).join(', ')); }); }, " +
  "function (e) { window.__axeOut = []; __probe.errs.push('axe rejected: ' + String(e && (e.stack || e.message) || e).slice(0, 300)); }), " +
  "false)) && Array.isArray(window.__axeOut)"
for (theme, lang) in looks {
  let q = "?rail=states&theme=\(theme)&lang=\(lang)"
  let rendered = railRendered(theme, lang)
  for (w, h) in sizes {
    var run = Run(name: "rail states \(theme) \(lang)", url: URL(string: "\(origin)/\(q)")!, width: w, height: h,
                  rendered: rendered, setup: "")
    run.measure = railMeasured
    runs.append(run)
  }
  // folded at 1100 px: Matters opens the rail as a floating panel over the work
  let folded = rendered + " && (window.__railPressed || (window.__railPressed = true," +
    " document.querySelector('.room-rail-toggle').click(), false))" +
    " && document.querySelector('nav.room-rail[data-open]') !== null" +
    " && document.querySelector('nav.room-rail').getBoundingClientRect().width > 0"
  var open = Run(name: "rail states \(theme) \(lang), folded and opened", url: URL(string: "\(origin)/\(q)")!,
                 width: 1100, height: 800, rendered: folded, setup: "")
  open.measure = railMeasured
  runs.append(open)
  runs.append(Run(name: "rail states \(theme) \(lang), frozen, axe", url: URL(string: "\(origin)/\(q)")!,
                  width: 1280, height: 800, rendered: rendered + axeRun,
                  setup: "Object.freeze(Object.prototype);"))
  // New matter, pressed: the dialog open over the room
  let dialog = rendered + " && (window.__newPressed || (window.__newPressed = true," +
    " document.querySelector('[data-testid=\"new-matter\"]').click(), false))" +
    " && document.querySelector('[role=\"dialog\"] [data-testid=\"matter-name\"]') !== null"
  var newMatter = Run(name: "new matter dialog \(theme) \(lang), frozen, axe", url: URL(string: "\(origin)/\(q)")!,
                      width: 1280, height: 800, rendered: dialog + axeRun,
                      setup: "Object.freeze(Object.prototype);")
  newMatter.measure = railMeasured
  runs.append(newMatter)
  // the scratch matter, opened from the foot: its row stands current, on the raised tone
  let scratch = rendered + " && (window.__scratchPressed || (window.__scratchPressed = true," +
    " document.querySelector('nav.room-rail [data-kind=\"scratch\"]').click(), false))" +
    " && document.querySelector('nav.room-rail [data-kind=\"scratch\"]').getAttribute('aria-current') === 'page'"
  var opened = Run(name: "scratch opened \(theme) \(lang)", url: URL(string: "\(origin)/\(q)")!, width: 1280,
                   height: 800, rendered: scratch, setup: "")
  opened.measure = "(function () { var errs = " + railMeasured + ", row = document.querySelector('nav.room-rail [data-kind=\"scratch\"]'), bg = getComputedStyle(row).backgroundColor;" +
    " if (bg === 'transparent' || bg === 'rgba(0, 0, 0, 0)') errs.push('layout: the open scratch matter row has no raised tone');" +
    " return errs; })()"
  runs.append(opened)
}

// the agent's permission requests (?thread=permission, probe-main.tsx): the room as every page has it, and in its thread a
// pending prompt on a paper with its four answers, above one answered once (data-state is the wire's, data-answer
// the kind chosen); measured as every page is and for prompts that never scroll sideways; and axe over it, under a
// frozen Object.prototype
let permissionMeasured = "(function () { var errs = " + measured + ";" +
  " document.querySelectorAll('[data-testid=\"permission\"]').forEach(function (p) {" +
  " if (p.scrollWidth > p.clientWidth + 1) errs.push('layout: a permission prompt scrolls sideways, ' + p.scrollWidth + ' px in ' + p.clientWidth); });" +
  " return errs; })()"
for (theme, lang) in looks {
  let q = "?thread=permission&theme=\(theme)&lang=\(lang)"
  let rendered = "document.querySelector('#root main.room-centre') !== null" +
    " && (document.querySelector('[data-testid=\"matter\"]')?.textContent ?? '') !== ''" +
    " && document.querySelectorAll('#root aside.room-pane li.tile').length === 5" +
    " && document.documentElement.dataset.theme === '\(theme)' && document.documentElement.lang === '\(lang)'" +
    " && (function (p) { return p.length === 2 && p[0].dataset.state === 'pending' && p[0].dataset.answer === undefined" +
    " && ['allow_once', 'allow_always', 'reject_once', 'reject_always'].every(function (k) {" +
    " return p[0].querySelector('[data-testid=\"permission-' + k + '\"]') !== null; })" +
    " && p[1].dataset.state === 'resolved' && p[1].dataset.answer === 'allow_once'; })" +
    "(document.querySelectorAll('[data-testid=\"permission\"]'))"
  // a running turn: Stop takes Ask's button, whose colour transition from a disabled Ask never advances in a WKWebView
  // with no window, so once rendered the page's transitions are finished (an endless animation can't be, and is left)
  // to their settled colours, for axe and the PNG; and the pending prompt is scrolled into view for the PNG, again as
  // the list lays out what it measured
  let settled = " && (window.__settled || (window.__settled = true," +
    " document.getAnimations().forEach(function (a) { try { a.finish(); } catch (e) {} })," +
    " [0, 300, 900].forEach(function (t) { setTimeout(function () {" +
    " document.querySelector('[data-testid=\"permission\"]').scrollIntoView({ block: 'center' }); }, t); }), true))"
  for (w, h) in sizes {
    var run = Run(name: "permission \(theme) \(lang)", url: URL(string: "\(origin)/\(q)")!, width: w, height: h,
                  rendered: rendered + settled, setup: "")
    run.measure = permissionMeasured
    runs.append(run)
  }
  var axe = Run(name: "permission \(theme) \(lang), frozen, axe", url: URL(string: "\(origin)/\(q)")!,
                width: 1280, height: 800, rendered: rendered + settled + axeRun,
                setup: "Object.freeze(Object.prototype);")
  axe.measure = permissionMeasured
  runs.append(axe)
  // Always for this matter, pressed, its answer held (?confirm=1): the OS's confirm line up and the answers waiting;
  // at the widest size, and under axe with Object.prototype frozen
  let cq = q + "&confirm=1"
  let confirming = rendered + settled + " && (window.__alwaysPressed || (window.__alwaysPressed = true," +
    " document.querySelector('[data-testid=\"permission-allow_always\"]').click(), false))" +
    " && (function (p) { return p.dataset.confirming !== undefined && p.querySelector('.perm-confirming') !== null" +
    " && p.querySelector('.perm-actions[inert]') === null" +
    " && p.querySelector('.perm-actions [aria-disabled=\"true\"]') !== null; })" +
    "(document.querySelector('[data-testid=\"permission\"]'))"
  var held = Run(name: "permission confirming \(theme) \(lang)", url: URL(string: "\(origin)/\(cq)")!, width: 1280,
                 height: 800, rendered: confirming, setup: "")
  held.measure = permissionMeasured
  runs.append(held)
  var heldAxe = Run(name: "permission confirming \(theme) \(lang), frozen, axe", url: URL(string: "\(origin)/\(cq)")!,
                    width: 1280, height: 800, rendered: confirming + axeRun, setup: "Object.freeze(Object.prototype);")
  heldAxe.measure = permissionMeasured
  runs.append(heldAxe)
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
    let js = "document.documentElement.getBoundingClientRect(); void document.body.offsetHeight; if (!(\(run.rendered))) return 0;" +
      " (\(run.measure)).forEach(function (e) { __probe.errs.push(e); }); return document.querySelectorAll('*').length;"
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
      if !errs.isEmpty { self.fail(errs.prefix(4).joined(separator: " | ")) } else { self.shoot(mine) }
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + budget) {
      if self.seq == mine && !back { self.fail("answered once, then stopped answering: hung") }
    }
  }

  /// With PROBE_SHOTS, a PNG of the page as WKWebView draws it, named for the run and its size; then the page passes.
  func shoot(_ mine: Int) {
    guard let dir = shots, let run = current, let view = web else { pass(); return }
    view.takeSnapshot(with: nil) { image, error in
      guard self.seq == mine else { return }
      let slug = "\(run.name) \(Int(run.width))x\(Int(run.height))".replacingOccurrences(of: ",", with: "")
        .replacingOccurrences(of: " ", with: "-")
      guard let image, let tiff = image.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff),
            let png = bitmap.representation(using: .png, properties: [:]) else {
        self.fail("no snapshot: \(error?.localizedDescription ?? "no image")")
        return
      }
      do {
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try png.write(to: dir.appendingPathComponent("\(slug).png"))
        self.pass()
      } catch {
        self.fail("could not save the snapshot: \(error.localizedDescription)")
      }
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
