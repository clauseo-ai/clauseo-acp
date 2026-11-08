//! chrome-headless-shell in the jail, as the browser guard runs it: a screenshot and the DOM of
//! a page in the matter. Needs a chrome-headless-shell this test does not download: set
//! `CLAUSEO_JAIL_CHROME` to its binary and run with `--ignored`.

use std::path::Path;

use clauseo_sandbox::BrowserSocketDir;

use crate::support::{Output, World, launcher, run};

/// Chrome's own sandbox cannot start inside the jail (Seatbelt refuses a second profile), so
/// the jail is the browser's sandbox and Chrome runs with `--no-sandbox`.
const ARGS: &[&str] = &[
    "--headless",
    "--no-sandbox",
    "--no-first-run",
    "--disable-background-networking",
    "--use-mock-keychain",
    "--password-store=basic",
    "--enable-unsafe-swiftshader",
    "--window-size=400,300",
];

#[test]
#[ignore = "needs chrome-headless-shell: set CLAUSEO_JAIL_CHROME and run with --ignored"]
fn chrome_headless_shell_renders_a_page_in_the_jail() {
    let chrome = std::env::var("CLAUSEO_JAIL_CHROME").expect("CLAUSEO_JAIL_CHROME");
    let chrome = dunce::canonicalize(chrome).unwrap();
    let w = World::new();
    w.file(
        "m1/page.html",
        "<html><body><h1>jail page</h1></body></html>\n",
    );
    let sockets = BrowserSocketDir::create().unwrap();
    let policy = w
        .builder()
        .runtime(chrome.parent().unwrap())
        .browser_socket_dir(&sockets)
        .build()
        .unwrap();
    let (launcher, _) = launcher(policy);
    let m1 = w.at("m1");
    let page = format!("file://{}", m1.join("page.html").display());
    let chrome_in = |extra: &[String]| -> Output {
        let mut args: Vec<String> = ARGS.iter().map(|a| (*a).to_owned()).collect();
        args.push(format!("--user-data-dir={}", m1.join("profile").display()));
        args.extend_from_slice(extra);
        args.push(page.clone());
        // A minute at most: a browser that hangs fails the test instead of holding it.
        let mut all = vec!["-e", "alarm 60; exec @ARGV", chrome.to_str().unwrap()];
        all.extend(args.iter().map(String::as_str));
        let home = m1.to_str().unwrap();
        run(&launcher, "/usr/bin/perl", &all, &m1, &[("HOME", home)])
    };

    let shot = m1.join("shot.png");
    let out = chrome_in(&[format!("--screenshot={}", shot.display())]);
    out.assert_ok();
    assert_png(&shot);
    let dom = chrome_in(&["--dump-dom".to_owned()]);
    assert!(
        dom.assert_ok().stdout.contains("<h1>jail page</h1>"),
        "{dom:#?}"
    );
}

#[track_caller]
fn assert_png(path: &Path) {
    let bytes = std::fs::read(path).unwrap();
    assert!(
        bytes.starts_with(b"\x89PNG"),
        "{} is not a PNG",
        path.display()
    );
    assert!(
        bytes.len() > 1000,
        "{} is {} bytes",
        path.display(),
        bytes.len()
    );
}
