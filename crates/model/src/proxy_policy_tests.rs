use super::*;

#[test]
fn loopback_detection_does_not_treat_similar_domains_or_private_networks_as_local() {
    for input in [
        "http://localhost:123/v1",
        "http://127.0.0.1",
        "http://127.1.2.3",
        "http://[::1]",
    ] {
        assert!(
            is_loopback_endpoint(&reqwest::Url::parse(input).expect("URL")),
            "{input}"
        );
    }
    for input in [
        "https://localhost.example.com",
        "http://192.168.1.1",
        "https://api.example.com",
        "http://[2001:db8::1]",
    ] {
        assert!(
            !is_loopback_endpoint(&reqwest::Url::parse(input).expect("URL")),
            "{input}"
        );
    }
}

#[test]
fn local_bypass_preserves_existing_exceptions_and_external_proxy_configuration() {
    let mut command = Command::new("unused-test-process");
    command
        .env("NO_PROXY", "private.example")
        .env("no_proxy", "legacy.example")
        .env("HTTP_PROXY", "http://proxy.example:8080");
    preserve_local_gateway_routing(&mut command);
    let values: std::collections::BTreeMap<_, _> = command.get_envs().collect();
    // Windows names are case-insensitive: the second assignment replaces the
    // first. Unix keeps both, so both sets of existing exceptions must survive.
    let expected = Some(OsStr::new(if cfg!(windows) {
        "127.0.0.1,localhost,::1,legacy.example"
    } else {
        "127.0.0.1,localhost,::1,private.example,legacy.example"
    }));
    assert_eq!(values[OsStr::new("NO_PROXY")], expected);
    #[cfg(not(windows))]
    assert_eq!(values[OsStr::new("no_proxy")], expected);
    assert_eq!(
        values[OsStr::new("HTTP_PROXY")],
        Some(OsStr::new("http://proxy.example:8080"))
    );
}
