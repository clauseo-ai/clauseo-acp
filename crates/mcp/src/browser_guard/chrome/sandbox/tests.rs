use std::ffi::OsStr;

use super::{
    DISABLE_FEATURES, ENABLE_FEATURES, FAILURES, Host, REFUSAL, SWITCHES, failure, host_args,
    refused_up_front, weakening, weakening_in,
};

/// Chrome's lines when its sandbox cannot start, as its source prints them (the files named in
/// `FAILURES`), with the log prefix Chrome puts on its own.
const FAILURE_LINES: &[&str] = &[
    "[1:1:0101/000000.000000:FATAL:zygote_host_impl_linux.cc(146)] No usable sandbox! If you are \
     running on Ubuntu 23.10+ or another Linux distro that has disabled unprivileged user \
     namespaces with AppArmor, see ...",
    "[1:1:0101/000000.000000:ERROR:zygote_host_impl_linux.cc(118)] Running as root without \
     --no-sandbox is not supported. See https://crbug.com/638180.",
    "[1:1:0101/000000.000000:FATAL:zygote_host_impl_linux.cc(244)] Zygote process exited \
     prematurely with exit code 1 (the namespace sandbox failed)",
    "[1:1:0101/000000.000000:FATAL:zygote_host_impl_linux.cc(248)] Failed to receive boot message \
     from zygote",
    "[1:1:0101/000000.000000:FATAL:zygote_host_impl_linux.cc(273)] Failed to receive hello \
     message from zygote",
    "Failed to move to new namespace: PID namespaces supported, Network namespace supported, but \
     failed: errno = Operation not permitted",
    "The setuid sandbox is not running as root. Common causes:",
    "[1:1:0101/000000.000000:FATAL:setuid_sandbox_host.cc(163)] The SUID sandbox helper binary is \
     missing: /opt/google/chrome/chrome-sandbox Aborting now.",
    "[1:1:0101/000000.000000:FATAL:setuid_sandbox_host.cc(173)] The SUID sandbox helper binary was \
     found, but is not configured correctly. Rather than run without sandboxing I'm aborting now.",
    "[1:1:0101/000000.000000:FATAL:sandbox_linux.cc(154)] Failed to instantiate the setuid \
     sandbox client.",
    "[1:1:0101/000000.000000:ERROR:content_main_runner_impl.cc(360)] Zygote cannot be disabled if \
     sandbox is enabled. Use --no-zygote together with --no-sandbox",
    "[1:1:0101/000000.000:ERROR:sandbox_win.cc(805)] Sandbox cannot access executable \
     C:\\Chrome\\chrome.exe. Check filesystem permissions are valid. See https://bit.ly/31yqMJR.: \
     Access is denied. (0x5)",
    "SeatbeltExecClient: pipe failed: Too many open files",
];

#[test]
fn every_sandbox_switch_is_refused_in_every_spelling_chrome_reads() {
    for switch in SWITCHES {
        let upper = switch.to_ascii_uppercase();
        for spelling in [
            format!("--{switch}"),
            format!("-{switch}"),
            format!("/{switch}"),
            format!("--{switch}=1"),
            format!("--{switch}="),
            format!("--{upper}"),
            format!("--{upper}=true"),
            format!("  --{switch}\t"),
            // Windows trims Unicode whitespace too (`TrimWhitespace`).
            format!("\u{3000}--{switch}\u{a0}"),
            format!("\u{2028}--{switch}\u{2003}"),
        ] {
            assert_eq!(
                weakening(OsStr::new(&spelling)).as_deref(),
                Some(*switch),
                "{spelling}"
            );
        }
    }
    // The ones the rule names, by name.
    for switch in [
        "--no-sandbox",
        "--disable-gpu-sandbox",
        "--no-zygote",
        "--disable-setuid-sandbox",
        "--single-process",
        "--disable-seccomp-filter-sandbox",
        "--disable-namespace-sandbox",
        "--in-process-gpu",
        "--No-Sandbox=1",
    ] {
        assert!(weakening(OsStr::new(switch)).is_some(), "{switch}");
    }
}

#[test]
fn a_feature_list_that_loosens_the_sandbox_is_refused() {
    for feature in DISABLE_FEATURES {
        for list in [
            format!("--disable-features={feature}"),
            format!(
                "--disable-features=Translate,{}",
                feature.to_ascii_uppercase()
            ),
            format!("--DISABLE-FEATURES=*{feature}<Study:param/1"),
        ] {
            assert!(weakening(OsStr::new(&list)).is_some(), "{list}");
        }
    }
    // A sandbox feature Chromium names later, by its words.
    for future in [
        "GpuSandboxV2",
        "WinSboxSomething",
        "XrAppContainer",
        "UtilityLpac",
    ] {
        let list = format!("--disable-features=Translate,{future}");
        assert!(weakening(OsStr::new(&list)).is_some(), "{list}");
    }
    for feature in ENABLE_FEATURES {
        let list = format!("--enable-features=NetworkService,{feature}");
        assert!(weakening(OsStr::new(&list)).is_some(), "{list}");
    }
    assert!(weakening(OsStr::new("--enable-features=NetworkServiceInProcess2")).is_some());
    // A service moved out of its own sandboxed process into the browser's, by name and by the
    // words of one Chromium adds later.
    for list in [
        "--disable-features=AudioServiceOutOfProcess",
        "--disable-features=Translate,*audioserviceoutofprocess<Study",
        "--disable-features=SpeechServiceOutOfProcess",
        "--disable-features=UseOutOfProcessVideoEncoding",
        "--enable-features=RunVideoCaptureServiceInBrowserProcess",
        "--enable-features=NetworkService,StorageServiceInProcess:param/1",
        "--enable-features=UseUnexportableKeyServiceInBrowserProcess",
    ] {
        assert!(weakening(OsStr::new(list)).is_some(), "{list}");
    }
    // Enabling a sandbox feature hardens it; a list with none passes.
    for fine in [
        "--enable-features=NetworkServiceSandbox,RendererAppContainer",
        "--disable-features=Translate,OptimizationHints",
        "--enable-features=NetworkService,WebMCPTesting",
        // Enabling the out-of-process service is what Chrome does by default.
        "--enable-features=AudioServiceOutOfProcess",
    ] {
        assert_eq!(weakening(OsStr::new(fine)), None, "{fine}");
    }
}

/// `--gpu-sandbox-failures-fatal` passes bare; with a value it is refused, since `=no` keeps GPU
/// sandbox failures non-fatal (`sandbox/policy/linux/sandbox_linux.cc`).
#[test]
fn gpu_sandbox_failures_fatal_passes_only_bare() {
    assert_eq!(weakening(OsStr::new("--gpu-sandbox-failures-fatal")), None);
    assert_eq!(
        weakening_in(OsStr::new("--gpu-sandbox-failures-fatal")),
        None
    );
    for refused in [
        "--gpu-sandbox-failures-fatal=no",
        "--GPU-Sandbox-Failures-Fatal=no",
        "--gpu-sandbox-failures-fatal=",
        "\u{3000}--gpu-sandbox-failures-fatal=yes",
    ] {
        assert!(weakening(OsStr::new(refused)).is_some(), "{refused}");
    }
    assert!(weakening_in(OsStr::new("--args=--gpu-sandbox-failures-fatal=no")).is_some());
}

#[test]
fn other_switches_values_and_arguments_pass() {
    for fine in [
        "--headless=new",
        "--user-data-dir=/tmp/no-sandbox",
        "--proxy-server=http://127.0.0.1:8080",
        "--disable-dev-shm-usage",
        "--no-sandbox-is-not-this",
        "about:blank",
        "no-sandbox",
    ] {
        assert_eq!(weakening(OsStr::new(fine)), None, "{fine}");
    }
}

#[test]
fn a_mention_anywhere_in_a_value_is_refused() {
    for switch in SWITCHES {
        let text = format!(
            "--args=--headless=new,--{},--mute-audio",
            switch.to_ascii_uppercase()
        );
        assert!(weakening_in(OsStr::new(&text)).is_some(), "{text}");
    }
    for text in [
        "--disable-features=Translate,NetworkServiceSandbox",
        "--disable-features=Translate\n--disable-features=GpuSandboxV2",
        "--enable-features=NetworkServiceInProcess2",
        "--disable-features=Translate,AudioServiceOutOfProcess",
        "--enable-features=RunVideoCaptureServiceInBrowserProcess",
    ] {
        assert!(weakening_in(OsStr::new(text)).is_some(), "{text}");
    }
    for fine in [
        super::super::env::WEBRTC_ARG,
        "mcp",
        "--tools",
        "core",
        "--enable-features=NetworkServiceSandbox",
    ] {
        assert_eq!(weakening_in(OsStr::new(fine)), None, "{fine}");
    }
}

#[test]
fn every_sandbox_failure_chrome_prints_is_recognised_and_ordinary_lines_are_not() {
    for line in FAILURE_LINES {
        assert!(failure(line).is_some(), "{line}");
    }
    for text in FAILURES {
        assert!(
            FAILURE_LINES
                .iter()
                .any(|line| failure(line) == Some(*text)),
            "no line for {text}"
        );
    }
    for line in [
        "DevTools listening on ws://127.0.0.1:61101/devtools/browser/abc",
        "[1:1:0101/000000.000000:ERROR:bus.cc(407)] Failed to connect to the bus",
        "[1:1:0101/000000.000000:WARNING:sandbox_linux.cc(420)] InitializeSandbox() called with \
         multiple threads in process gpu-process.",
        "Cannot start http server for devtools.",
    ] {
        assert_eq!(failure(line), None, "{line}");
    }
}

#[test]
fn linux_as_root_is_refused_and_no_host_gets_a_weakening_switch() {
    for linux in [false, true] {
        for root in [false, true] {
            for container in [false, true] {
                let host = Host {
                    linux,
                    root,
                    container,
                };
                assert_eq!(refused_up_front(host), linux && root, "{host:?}");
                for arg in host_args(host) {
                    assert_eq!(weakening(OsStr::new(arg)), None, "{arg} for {host:?}");
                    assert_eq!(weakening_in(OsStr::new(arg)), None, "{arg} for {host:?}");
                }
                let shm = host_args(host).contains(&"--disable-dev-shm-usage");
                assert_eq!(shm, linux && (root || container), "{host:?}");
                let fatal = host_args(host).contains(&"--gpu-sandbox-failures-fatal");
                assert_eq!(fatal, linux, "{host:?}");
            }
        }
    }
}

#[test]
fn the_refusal_is_the_plain_sentence() {
    assert_eq!(
        REFUSAL,
        "The browser couldn't start its own protection on this computer, so Clauseo won't use it."
    );
}
