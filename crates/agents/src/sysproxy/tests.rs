use super::*;

#[test]
fn scutil_s_https_proxy_is_read_only_when_enabled() {
    let on = "<dictionary> {\n  ExceptionsList : <array> {\n    0 : *.local\n  }\n  \
              HTTPEnable : 1\n  HTTPPort : 3128\n  HTTPProxy : web.firm.example\n  \
              HTTPSEnable : 1\n  HTTPSPort : 8443\n  HTTPSProxy : proxy.firm.example\n}\n";
    assert_eq!(parse_scutil(on).as_deref(), Some("proxy.firm.example:8443"));
    let no_port = "<dictionary> {\n  HTTPSEnable : 1\n  HTTPSProxy : 10.0.0.2\n}\n";
    assert_eq!(parse_scutil(no_port).as_deref(), Some("10.0.0.2"));
    let off = on.replace("HTTPSEnable : 1", "HTTPSEnable : 0");
    assert_eq!(parse_scutil(&off), None);
    let http_only = "<dictionary> {\n  HTTPEnable : 1\n  HTTPProxy : web.firm.example\n}\n";
    assert_eq!(parse_scutil(http_only), None);
    assert_eq!(parse_scutil("<dictionary> {\n}\n"), None);
    assert_eq!(parse_scutil(""), None);
}

#[test]
fn internet_settings_take_one_server_or_the_https_entry() {
    assert_eq!(
        parse_proxy_server(1, "proxy.firm.example:8080").as_deref(),
        Some("proxy.firm.example:8080")
    );
    assert_eq!(
        parse_proxy_server(1, "http=web:80;https=secure.firm.example:8443;ftp=f:21").as_deref(),
        Some("secure.firm.example:8443")
    );
    assert_eq!(
        parse_proxy_server(1, "HTTPS=http://secure:8443/").as_deref(),
        Some("secure:8443")
    );
    assert_eq!(parse_proxy_server(1, "http=web:80;socks=s:1080"), None);
    assert_eq!(parse_proxy_server(0, "proxy.firm.example:8080"), None);
    assert_eq!(parse_proxy_server(1, "  "), None);
}

#[cfg(unix)]
mod run {
    use clauseo_process::{AsyncIoTimer, ManualTimer, PassthroughLauncher, SpawnOptions};
    use futures::executor::block_on;
    use futures::future::join;

    use super::*;

    #[test]
    fn a_program_s_stdout_is_read_and_parsed() {
        let launcher = PassthroughLauncher::new(SpawnOptions::new());
        let script = "printf 'HTTPSEnable : 1\\nHTTPSProxy : p.example\\nHTTPSPort : 3128\\n'";
        let out = block_on(run_stdout(
            &launcher,
            &AsyncIoTimer,
            "/bin/sh",
            &["-c", script],
            Duration::from_secs(5),
        ));
        assert_eq!(
            parse_scutil(&out.unwrap()).as_deref(),
            Some("p.example:3128")
        );
        let failed = block_on(run_stdout(
            &launcher,
            &AsyncIoTimer,
            "/bin/sh",
            &["-c", "exit 3"],
            Duration::from_secs(5),
        ));
        assert_eq!(failed, None);
    }

    #[test]
    fn a_program_past_its_limit_gives_nothing() {
        let launcher = PassthroughLauncher::new(SpawnOptions::new());
        let timer = ManualTimer::new();
        let (out, ()) = block_on(join(
            run_stdout(
                &launcher,
                &timer,
                "/bin/sh",
                &["-c", "exec sleep 30"],
                Duration::from_secs(5),
            ),
            async {
                assert_eq!(timer.next_armed().await, Duration::from_secs(5));
                assert!(timer.fire());
            },
        ));
        assert_eq!(out, None);
    }
}
